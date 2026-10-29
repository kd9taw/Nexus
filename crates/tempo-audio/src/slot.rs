//! The per-slot transmit/receive decision — the heart of the radio loop, split
//! out of `service.rs::run_radio` so it is unit-testable with a `MockBackend`
//! (and a VOX/mock rig) and needs no sound card. This is a behavior-preserving
//! extraction of the slot core; the device/network/tune machinery stays in
//! `run_radio`.

use tempo_app::engine::Engine;
use tempo_core::tempo_fast;
use tempo_core::timing::now_unix_ms;

use crate::backend::AudioBackend;
use crate::frames::RxRing;
use crate::rig::Rig;

/// PTT-hold tail after the transmitted audio plays out (ms) — covers ring
/// drain + relay release so the start of RX isn't clipped by our own carrier.
/// Every slot tier but FT8 and FT4, which end on WSJT-X's own window ([`wsjtx_over_end`]).
pub const TX_TAIL_MS: f64 = 250.0;

/// The PTT-hold deadline for an over keyed at `keyed_ms`, **bounded by the slot it
/// was keyed in**.
///
/// ⭐ THIS BOUND IS WSJT-X'S, AND WE HAD NOTHING LIKE IT. Upstream computes no
/// deadline at all: it clamps the window to the period and re-tests position
/// continuously, every GUI tick (mainwindow.cpp):
///
/// ```text
/// double tx1=0.0;
/// double tx2=txDuration;
/// if(tx2>m_TRperiod) tx2=m_TRperiod;        // clamped to the period
/// m_bTxTime = (t2p >= tx1) and (t2p < tx2); // t2p = position WITHIN the period
/// ```
///
/// Because `t2p` is slot-relative and `tx2` is clamped, **an upstream over can
/// never cross the boundary** however late keying starts. Ours was an ABSOLUTE
/// deadline `keyed + audio + tail` with no slot term, so keying latency — a
/// blocking CAT exchange can spend a full 700 ms deadline, 2500 ms on a slow
/// transport — converted directly into slot overrun.
///
/// FT8 and FT4 hide that: 12.64 s of audio in a 15 s slot and 5.04 s in 7.5 s
/// leave ~2.11 s and ~2.21 s of slack, more than any single CAT stall.
///
/// ⚠️ **FT1 HAS −250 ms, AND IS THEREFORE CLAMPED ON EVERY OVER, AT EVERY PHASE — INCLUDING ONE
/// KEYED EXACTLY ON THE BOUNDARY.** An earlier version of this doc said "214 ms", computed from
/// FT1's TONE length (99 symbols x 3000/7 samples = 3.5357 s). That is not what the deadline is
/// sized from. `tempo_fast::gen_wave` returns a FIXED NMAX = 48000-sample (4.0000 s) buffer —
/// tones plus trailing zeros, and `Ft1Mode::gen_wave` shifts the lead-in WITHIN it rather than
/// prepending, precisely to keep the length at 4.0000 s (crates/modes/src/mode.rs). The hold is
/// sized from `w.len()`, so the real arithmetic is 4.000 + 0.250 = 4.250 in a 4.000 s period.
/// FT1's effective post-tone tail is a fixed **64.2 ms** (tones end at 0.400 + 3.5357 = 3.9357 s;
/// the clamp drops PTT at 4.000 s).
///
/// **RESOLVED 2026-08-05 by shortening the LEAD, on the operator's decision.** At the former
/// 0.400 s lead the tail was 64 ms, and because the device output ring lags the buffer by the
/// soundcard's latency, any output latency above that cut FT1's RF mid-tone — inside the failure
/// region for ordinary 20-100 ms cpal buffers. The lead is now 0.300 s
/// (`FT1_LEAD_IN_SECS`, crates/modes/src/mode.rs) and the tail is **164 ms**.
///
/// Lead and tail come out of one 464 ms budget and trade one-for-one, so this cost 100 ms of
/// early-timing tolerance — measured as a hard cliff, not a slope, in
/// `crates/tempo-fast/tests/ft1_lead_sweep.rs`. FT1 still cannot have both a full lead and a full
/// 250 ms tail; 0.300/0.164 is the balance chosen, and the sweep is the instrument for revisiting
/// it. The clamp itself is unchanged and still fires on every FT1 over. The 4 s period is by design (operator,
/// 2026-08-04: "ft1 has a designed 4 sec timing… but it should still follow strict
/// wsjtx based aspects of the transmission"); the missing bound was the defect.
///
/// The boundary is derived from `keyed_ms` rather than from a caller-supplied slot
/// index so it cannot disagree with the instant it bounds — same arithmetic as
/// `SlotClock::next_boundary_ms`, and the same "position within the period"
/// semantics upstream uses.
///
/// Clamping can only make PTT drop EARLIER, never later, so it cannot weaken the
/// transmit-path safety invariants. Floored at `keyed_ms` so a hold is never
/// negative: an over keyed past its own boundary releases at once rather than
/// carrying a deadline in the past.
///
/// FT8 and FT4 no longer end here: they end on WSJT-X's window itself, measured from the slot
/// rather than the key ([`wsjtx_over_end`]), with this same clamp. Every other tier still does.
pub fn tx_deadline_ms(keyed_ms: f64, audio_ms: f64, period_ms: f64) -> f64 {
    let natural_end = keyed_ms + audio_ms + TX_TAIL_MS;
    if period_ms.is_nan() || period_ms <= 0.0 {
        return natural_end; // no period to bound against
    }
    let boundary = (keyed_ms / period_ms).floor() * period_ms + period_ms;
    natural_end.min(boundary).max(keyed_ms)
}

/// WSJT-X's delay from the end of an FT8/FT4 over's audio to PTT off: `stopTx` stops the modulator
/// and starts `ptt0Timer.start(200)`, whose timeout lowers PTT (`widgets/mainwindow.cpp:5743-5760`,
/// upstream master).
pub const WSJTX_PTT_OFF_DELAY_MS: f64 = 200.0;

/// WSJT-X's transmit window for an FT8 or FT4 over, in ms from the start of its slot; `None` for
/// every other tier, which keep [`tx_deadline_ms`].
///
/// WSJT-X computes no deadline. Every 100 ms GUI tick it asks whether the slot position is inside
/// `[0, txDuration)` (`m_bTxTime = (t2p >= tx1) and (t2p < tx2)`, `widgets/mainwindow.cpp:5055`), and
/// the tick that finds it outside stops the audio (`:5180`, `:5530`). `txDuration` is
/// `tx_duration()` (`helper_functions.cpp:6-7`): FT8 `1.0 + 79*1920/12000` = 13.64 s, FT4
/// `1.0 + 105*576/12000` = 6.04 s, the tones plus one second whatever the over's lead-in.
pub fn wsjtx_tx_window_ms(tier: tempo_app::dto::Tier) -> Option<f64> {
    use tempo_app::dto::Tier;
    match tier {
        Tier::Ft8 => Some(1_000.0 + 79.0 * 1_920.0 / 12.0),
        Tier::Ft4 => Some(1_000.0 + 105.0 * 576.0 / 12.0),
        _ => None,
    }
}

/// WSJT-X's latest key for an FT8 or FT4 over: PTT is asserted only while the slot is under 75 % of
/// its period, FT8 11.25 s and FT4 5.625 s. The GUI tick that asserts it (`widgets/mainwindow.cpp:5114`
/// and `:5126`, upstream master):
///
/// ```text
/// double fTR=float((ms%int(1000.0*m_TRperiod)))/int(1000.0*m_TRperiod);
/// if(g_iptt==0 and ((m_bTxTime and (fTR < 0.75) and (msgLength>0)) or m_tune)) {
///   //### Allow late starts
/// ```
///
/// Under it a late key sends the symbols that remain, on the slot clock ([`play_keyed_over`]); at or
/// past it the over waits for this station's next slot. One rule for both of Nexus's late keys: a Call
/// CQ or a double-click inside this station's own slot (the radio loop's late start), and a boundary
/// whose key comes late ([`slot_tx_phase`]). `into_slot_ms` is how far into its slot the key would
/// be ([`into_tick_slot_ms`]).
pub fn wsjtx_may_key_at(into_slot_ms: f64, period_ms: f64) -> bool {
    into_slot_ms < 0.75 * period_ms
}

/// How far into the slot of a tick whose clock read `now_ms` a key decided now would be. `reread_ms`
/// is the steered clock read again at the decision, which carries whatever the tick spent before it
/// (CAT, a decode's wait), placed on the tick's slot clock as [`slot_clock_at`] places it. Measured
/// from the start of the tick's slot, so a re-read that has crossed into the next slot, the other
/// station's, reads past the period: too late for this one.
pub(crate) fn into_tick_slot_ms(reread_ms: f64, now_ms: f64, period_ms: f64) -> f64 {
    slot_clock_at(reread_ms.max(now_ms), now_ms, period_ms)
        - (now_ms / period_ms).floor() * period_ms
}

/// Where and when a tick decided to key an FT8 or FT4 over: how far into the tick's slot
/// ([`into_tick_slot_ms`]) and the instant, on the monotonic clock. It is the last slot check
/// before the key, and the key's own lateness is measured from it ([`Self::past_its_period`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyDecision {
    pub into_slot_ms: f64,
    pub at: std::time::Instant,
}

impl KeyDecision {
    /// The decision a tick whose clock read `now_ms` makes now, the steered clock read again as
    /// `reread_ms`.
    pub(crate) fn now(reread_ms: f64, now_ms: f64, period_ms: f64) -> KeyDecision {
        KeyDecision {
            into_slot_ms: into_tick_slot_ms(reread_ms, now_ms, period_ms),
            at: std::time::Instant::now(),
        }
    }

    /// How far past the end of the slot this decision was for a carrier that came up now is, or
    /// `None` while it is still inside that slot.
    ///
    /// ⭐ MEASURED ON THE MONOTONIC CLOCK FROM THE DECISION, NOT FROM A RE-READ OF THE WALL CLOCK.
    /// Everything after the last slot check is in it: the build, Split Operation's CAT (three
    /// commands on a slow link, each up to its 2.5 s deadline), the sound card's open and the key's
    /// own round trip. A wall-clock re-read places the carrier as the tones are placed
    /// ([`slot_clock_at`]), which takes the tick's clock when the two are a period or more apart:
    /// an FT4 key 8 s late read as on time there. Measured from the decision it reads as what it
    /// is, on a loop driven on a clock of its own too.
    pub(crate) fn past_its_period(&self, period_ms: f64) -> Option<f64> {
        let into_slot_ms = self.into_slot_ms + self.at.elapsed().as_secs_f64() * 1000.0;
        (into_slot_ms >= period_ms).then_some(into_slot_ms - period_ms)
    }
}

/// When an FT8/FT4 over stops its audio and drops PTT ([`wsjtx_over_end`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverEnd {
    /// The output is flushed: what is still queued is dropped, as WSJT-X's stop drops what its
    /// audio output still holds (`Modulator::stop` → `SoundOutput::stop`).
    pub audio_stop_ms: f64,
    /// PTT drops.
    pub ptt_off_ms: f64,
}

/// WSJT-X's end for an FT8/FT4 over keyed at `keyed_ms`: the audio stops at the end of the window
/// ([`wsjtx_tx_window_ms`]) measured from the start of the slot the over was keyed in, and PTT drops
/// [`WSJTX_PTT_OFF_DELAY_MS`] later. Both are on the SLOT clock, so neither moves with keying latency
/// or with what the key cost.
///
/// The window is clamped to the period as WSJT-X clamps it (`if(tx2>m_TRperiod) tx2=m_TRperiod`,
/// `widgets/mainwindow.cpp:5015`), and PTT keeps [`tx_deadline_ms`]'s bound: never past the boundary
/// of the slot it was keyed in, never before the key. Neither binds for FT8 (13.84 s of 15) or FT4
/// (6.24 s of 7.5); they are what make an overrun unrepresentable rather than unlikely.
pub fn wsjtx_over_end(keyed_ms: f64, window_ms: f64, period_ms: f64) -> OverEnd {
    if period_ms.is_nan() || period_ms <= 0.0 {
        // No period to place the window in: hold from the key, as `tx_deadline_ms` falls back.
        let audio_stop_ms = keyed_ms + window_ms;
        return OverEnd {
            audio_stop_ms,
            ptt_off_ms: audio_stop_ms + WSJTX_PTT_OFF_DELAY_MS,
        };
    }
    let slot_start = (keyed_ms / period_ms).floor() * period_ms;
    let window_end = slot_start + window_ms.min(period_ms);
    let ptt_off_ms = (window_end + WSJTX_PTT_OFF_DELAY_MS)
        .min(slot_start + period_ms)
        .max(keyed_ms);
    OverEnd {
        audio_stop_ms: window_end.clamp(keyed_ms, ptt_off_ms),
        ptt_off_ms,
    }
}

/// How many samples of a SLOT-POSITIONED wave have gone by at `at_ms`. Sample 0 is the start of the
/// slot (FT8 and FT4 carry their 0.5 s lead-in at the head, `crates/modes/src/mode.rs`), so playing
/// from here puts every tone where an over keyed exactly on the boundary would have it.
///
/// That is WSJT-X's Modulator: it measures how far into the period it is starting (`mstr`), then
/// sends silence up to the tones' nominal start, or skips the symbols a late start has passed
/// (`m_silentFrames`, `m_ic`, `Modulator/Modulator.cpp:56-57, 85-101`). Here the lead-in is that
/// silence, so one skip does both: an early start keeps the rest of the lead-in, a late one loses
/// its first symbols, and either way the tones end where an on-time over's end.
pub fn slot_clock_skip(at_ms: f64, period_ms: f64) -> usize {
    if period_ms.is_nan() || period_ms <= 0.0 {
        return 0;
    }
    let into_slot_ms = at_ms - (at_ms / period_ms).floor() * period_ms;
    (into_slot_ms / 1000.0 * tempo_fast::SAMPLE_RATE as f64).round() as usize
}

/// `keyed_ms` (the steered wall clock after the key) on the slot clock of the caller, whose own
/// clock read `now_ms` earlier in the tick. In the live loop they are one clock and `keyed_ms` is
/// returned: it carries everything the tick spent before the key and on it. When they disagree by a
/// period or more, `now_ms` is the slot clock and is returned: a loop driven on a clock of its own
/// (the loop's tests run it at synthetic times), or a wall-clock step since `now_ms` was read, which
/// the loop's clock-step check acts on at its next tick.
fn slot_clock_at(keyed_ms: f64, now_ms: f64, period_ms: f64) -> f64 {
    if (0.0..period_ms).contains(&(keyed_ms - now_ms)) {
        keyed_ms
    } else {
        now_ms
    }
}

/// Whether a slot-positioned over still has a tone to send once `skip` samples of it have gone by.
fn tones_after(waves: &[Vec<f32>], mut skip: usize) -> bool {
    for w in waves {
        if w.iter().skip(skip).any(|&x| x != 0.0) {
            return true;
        }
        skip = skip.saturating_sub(w.len());
    }
    false
}

/// When a keyed slot over ends, from [`play_keyed_over`].
pub(crate) struct KeyedOver {
    /// When PTT drops.
    pub tx_until_ms: f64,
    /// FT8/FT4: when its audio stops, before PTT ([`wsjtx_over_end`]). `None` for every other tier.
    pub audio_until_ms: Option<f64>,
}

/// Hand a keyed slot over to the output and say when it ends. `keyed_ms` is the steered clock read
/// after the key; `now_ms` the clock the caller's tick read (see [`slot_clock_at`]).
///
/// FT8 and FT4 as WSJT-X: the tones on the slot clock ([`slot_clock_skip`]) and the over ended on
/// WSJT-X's window ([`wsjtx_over_end`]). An over whose every tone lies behind the key plays nothing
/// and is still held to the window's end, as WSJT-X holds it. Every other tier as it always did: the
/// whole wave from the key, held to [`tx_deadline_ms`].
pub(crate) fn play_keyed_over(
    eng: &Engine,
    backend: &mut impl AudioBackend,
    waves: &[Vec<f32>],
    keyed_ms: f64,
    now_ms: f64,
) -> KeyedOver {
    let period_ms = eng.active_slot_secs() * 1000.0;
    let Some(window_ms) = wsjtx_tx_window_ms(eng.tier()) else {
        let mut secs = 0.0f32;
        for w in waves {
            secs += w.len() as f32 / tempo_fast::SAMPLE_RATE;
            backend.play(w);
        }
        return KeyedOver {
            tx_until_ms: tx_deadline_ms(keyed_ms, secs as f64 * 1000.0, period_ms),
            audio_until_ms: None,
        };
    };
    let at_ms = slot_clock_at(keyed_ms, now_ms, period_ms);
    let mut skip = slot_clock_skip(at_ms, period_ms);
    let end = wsjtx_over_end(at_ms, window_ms, period_ms);
    let on_the_window = KeyedOver {
        tx_until_ms: end.ptt_off_ms,
        audio_until_ms: Some(end.audio_stop_ms),
    };
    if !tones_after(waves, skip) {
        // ⭐ AN FT8 OR FT4 OVER KEYED WITH NO TONE LEFT IS HELD TO THE WINDOW'S END ALL THE SAME,
        // SENDING NOTHING, AS WSJT-X HOLDS IT. FT4's tones end 5.54 s into the slot (the 0.5 s
        // lead-in), before its latest key at 5.625 s ([`wsjtx_may_key_at`]): a late start or a
        // boundary decided between the two has nothing left to send ([`slot_tx_phase`]). So has any
        // key in time whose carrier comes up after the last tone: FT8's tones run to 13.14 s, so more
        // than 1.89 s after a decision by 11.25 s (split's CAT on a slow link, a late start's build
        // waiting for a decode to let go of the modem). WSJT-X asserts PTT on the tick that decided
        // (`widgets/mainwindow.cpp:5126-5176`) and starts its Modulator when the radio answers
        // (`:8845-8856`, `startTx2` `:5711`); the Modulator finds `m_ic` past the last symbol and
        // sends none (`Modulator/Modulator.cpp:96-99`, `:281`). PTT falls only at the window's end
        // or a stop (`widgets/mainwindow.cpp:5180`, `:5530`, `stopTx` `:5743-5753`, `stopTx2`
        // `:5758-5760`), never because the Modulator ran out (the main window's only links to it run
        // the other way, `:525-528`): FT8's at 13.84 s, FT4's at 6.24 s. Such an over dropped PTT on
        // the next tick. The hold is the over's own, so Stop TX, TX Off and the watchdog end it as
        // they end any over, and a carrier that comes after the window's end still drops at once
        // ([`wsjtx_over_end`] never ends an over before its key).
        return on_the_window;
    }
    for w in waves {
        let rest = &w[skip.min(w.len())..];
        skip = skip.saturating_sub(w.len());
        if !rest.is_empty() {
            backend.play(rest);
        }
    }
    on_the_window
}

/// What a slot did, for the caller to thread back into loop state + reporting.
pub struct SlotAction {
    /// Set when we transmitted: hold PTT until this Unix-ms deadline.
    pub tx_until_ms: Option<f64>,
    /// FT8/FT4: when the over's audio stops, WSJT-X's window ending before its PTT does
    /// ([`wsjtx_over_end`]). `None` for every other tier, and when nothing keyed.
    pub audio_until_ms: Option<f64>,
    /// True when we decoded a receive frame into the engine this slot.
    pub did_rx: bool,
    /// The decoded period's samples (moved out, no extra copy) — the loop saves
    /// them as a WAV when settings.save_wav asks. None when nothing was decoded.
    pub rx_frame: Option<Vec<f32>>,
    /// True when we transmitted this slot — the next boundary uses this as
    /// `prev_was_tx` so it knows the capture ring then holds our own carrier.
    pub tx_this_slot: bool,
    /// Fake-It split moved the VFO for this over — restore it to this dial
    /// (Hz) once the over finishes playing (the loop owns the PTT deadline).
    pub fake_it_restore: Option<u64>,
    /// …and the TX dial it moved the VFO to (Hz), which a dial read must never take for the
    /// operator's QSY until the radio is read back off it.
    pub fake_it_tx_dial: Option<u64>,
    /// Rig-mode split engaged VFO B for this over — the loop tears the rig
    /// split down once the over ends (it would otherwise stay latched and a
    /// later in-window over would TX on a stale VFO B frequency).
    pub rig_split_engaged: bool,
}

/// What the Split-Operation pre-key step did, for the loop's teardown.
pub(crate) struct SplitApply {
    pub fake_it_restore: Option<u64>,
    pub fake_it_tx_dial: Option<u64>,
    pub rig_split_engaged: bool,
    /// The radio did not take a TX-dial write (refused, or no answer by the CAT deadline), in the
    /// rig link's words: the over must not key ([`Engine::halt_tx_for_refused_split`]). The
    /// fields above still say what to put back, as after any over.
    pub refused: Option<String>,
}

/// Apply the WSJT-X Split-Operation dial shift for the over about to key (must
/// run BEFORE PTT): `Rig` = shifted TX dial on VFO B (rig split); `FakeIt` =
/// retune the single VFO. Reports what engaged so the loop restores/tears down
/// at over end. No-op when the engine left shift = 0 and no Rig split is held.
///
/// ⭐ RIG SPLIT IS TURNED ON ONCE AND HELD, AS WSJT-X HOLDS IT. WSJT-X sets split on the first
/// time (`Transceiver/HamlibTransceiver.cpp:965-987`) and after that writes the TX frequency at
/// every key (`TransceiverBase.cpp:160-171`, "or about to tx split"); `setXIT` keeps a TX
/// frequency set while Split Operation is on, XIT 0 included (`widgets/mainwindow.cpp:12378-12387`),
/// so nothing turns split off between overs. `S 1 VFOB` goes out only while this connection does
/// not hold split ([`Rig::tx_split_held`]); the TX dial and its mode go out before every key, and
/// an over that needs no shift while split is held writes the RX dial, as WSJT-X writes XIT 0's,
/// never leaving VFO B on the last shifted dial. The radio loop lets go of the split
/// (`RadioLoop::let_go_of_rig_split`).
///
/// ⛔ A WRITE THE RADIO DOES NOT TAKE KEYS NOTHING ([`SplitApply::refused`]). The over's audio was
/// already moved into 1500-2000 Hz for the shifted dial, so a dial that never moved sends the over
/// 500 Hz or more from where the operator put it. WSJT-X's `error_check` throws on anything but
/// `RIG_OK` (`HamlibTransceiver.cpp:277-284`; split writes `:1022`, `:1027`, `:1034`) before
/// `do_ptt(true)`, and its rig-failure handler presses Halt Tx (`mainwindow.cpp:12556-12564`). An
/// answer that never says `RPRT 0` (a refusal, an empty reply, no reply by the CAT deadline) is
/// such a write; the writes after it are not sent. One exception, the TX VFO's mode (`X`) on a link
/// that has no way to set it (`RPRT -11`): the TX dial was taken, so the over keys as it always did.
pub(crate) fn apply_tx_dial_shift(eng: &mut Engine, rig: &mut Rig) -> SplitApply {
    use tempo_app::settings::SplitMode;
    let none = SplitApply {
        fake_it_restore: None,
        fake_it_tx_dial: None,
        rig_split_engaged: false,
        refused: None,
    };
    let shift = eng.take_tx_dial_shift();
    let held = rig.tx_split_held
        && eng.settings().split_mode == SplitMode::Rig
        && !eng.cluster_split_active();
    if shift == 0 && !held {
        return none;
    }
    let dial = eng.settings().dial_hz();
    let tx_dial = (dial as i64 + shift).max(0) as u64;
    match eng.settings().split_mode {
        SplitMode::Rig => {
            // …AND THE TX VFO'S MODE. `M` only ever reaches the RX VFO, so without the `X` below VFO
            // B transmits in whatever mode it was last left in — reported on an FTDX1200 and then
            // again on an FTDX10, where FT8 went out of a VFO that was not in the data submode
            // and nothing on screen said so. WSJT-X does exactly this when split is on
            // (HamlibTransceiver.cpp:922 `rig_set_mode(rig, RIG_VFO_B, mode, RIG_PASSBAND_NOCHANGE)`
            // and :1093 `rig_set_split_mode(..., RIG_PASSBAND_NOCHANGE)`).
            //
            // ⚠️ WIDTH IS NOCHANGE (-1), NOT `passband_for(md)`. WSJT-X passes NOCHANGE, and the
            // point here is the MODE — sending a width would reach across and reset the filter
            // the operator set on the TX VFO, which is the shape of the very bug #67 was about.
            // AFTER the split enable: `X` addresses the TX VFO, so it has to exist first.
            let md = eng.rig_mode_effective();
            let sent = (|| {
                if !rig.tx_split_held {
                    rig.set_split(true, "VFOB")?;
                    rig.tx_split_held = true;
                }
                rig.set_split_freq(tx_dial)?;
                // A link that cannot set the TX VFO's mode at all (`RPRT -11`: Nexus's own TCI
                // client) keys as it always did, its TX dial taken; any other refusal, or no
                // answer, keys nothing.
                rig.set_split_mode_where_available(&md, -1).map(|_| ())
            })();
            SplitApply {
                rig_split_engaged: true,
                refused: sent.err().map(|e| e.to_string()),
                ..none
            }
        }
        SplitMode::FakeIt => SplitApply {
            fake_it_restore: Some(dial),
            fake_it_tx_dial: Some(tx_dial),
            refused: rig.set_freq(tx_dial).err().map(|e| e.to_string()),
            ..none
        },
        SplitMode::None => none, // shift can't be non-zero here, but stay total
    }
}

/// Check Remote authority on both sides of blocking PTT I/O. Native TX has
/// no Remote permit and keeps its established keying behavior. A loss during
/// key-up is unkeyed before any waveform reaches the output device.
///
/// ⛔ A KEY THE RADIO DID NOT ACCEPT SENDS NOTHING AND HALTS TX, AS WSJT-X DOES
/// ([`slot_key_failure`]). The key's answer used to be ignored, so a radio that said no had the
/// whole over played into it while it was receiving, and TX stayed armed to try again next cycle.
/// Now the caller plays nothing, and [`Engine::halt_tx_for_refused_key`] presses Stop TX, whose
/// slot abort the loop's next tick turns into the unkey and the flush, and says why. `held`: the
/// loop already holds the transmitter (an over's PTT hold, or the operator's PTT), so this key is
/// not a new one. A key the radio accepts is untouched: nothing new runs between its answer and the
/// audio.
///
/// ⛔ AN OVER WITH NO WAY TO ITS AUDIO NEVER KEYS AT ALL. The sound card's output was opened by the
/// over's own `play`, after the key, so an output that would not open put a dead carrier on the air
/// for the whole over, and for every over after it. The radio loop now opens it when slot TX is
/// armed ([`AudioBackend::prepare_tx`]); here, before any PTT command, it is asked once more (a
/// check, once it is open) and an `Err` keys nothing: [`Engine::halt_tx_for_tx_audio`] halts TX and
/// keeps the sound card's reason on screen.
pub(crate) fn key_slot_transmitter(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    held: bool,
) -> bool {
    if eng.poll_remote_transmit(std::time::Instant::now()) {
        if rig.keyed {
            let _ = rig.ptt(false);
        }
        backend.flush_output();
        return false;
    }
    if let Err(why) = backend.prepare_tx() {
        eng.halt_tx_for_tx_audio(&why);
        return false;
    }
    let key = rig.ptt(true);
    if eng.poll_remote_transmit(std::time::Instant::now()) {
        let _ = rig.ptt(false);
        backend.flush_output();
        return false;
    }
    if let Some(why) = slot_key_failure(&key, held) {
        eng.halt_tx_for_refused_key(&why);
        return false;
    }
    true
}

/// ⛔ AN FT8/FT4 KEY THE RADIO ANSWERED PAST THE END OF ITS SLOT SENDS NOTHING AND IS UNKEYED AT
/// ONCE ([`KeyDecision::past_its_period`]), what the caller does instead of playing the over. Its
/// tones were laid on the clock where the carrier was, the other station's slot clock, so the rest
/// of the over went out in their period, ending on their window. WSJT-X starts an over's audio only
/// when the radio reports PTT on and the over is "still needed" (`widgets/mainwindow.cpp:8852`,
/// `m_tx_when_ready && g_iptt`); its 100 ms GUI tick ends the over as soon as the clock leaves its
/// window (`:870`, `:5055`, `:5180`, `:5530`), a window in this station's own period of the pair
/// (`:5016-5019`), and `stopTx` clears `g_iptt` and lowers PTT 200 ms later (`:5743-5760`). A
/// radio that answers in the other station's period sends nothing there and is unkeyed.
///
/// The output is flushed (it holds nothing of this over); the radio is unkeyed now, unless the loop
/// already held the transmitter (`held`: the operator's own PTT stays theirs, and an over's hold
/// ends at its own deadline); and only this over is dropped
/// ([`Engine::drop_over_keyed_past_its_period`]): TX stays on and the next own period keys its over
/// as usual, however many in a row, as WSJT-X's `stopTx` ends that over alone and leaves Enable Tx
/// ticked, with a lane of its own saying so. An unkey the radio does not take halts TX as at an
/// over's end ([`slot_unkey_failure`]), and the loop's idle self-heal goes on sending it.
pub(crate) fn unkey_a_key_past_its_period(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    held: bool,
    decided: KeyDecision,
    past_ms: f64,
) {
    backend.flush_output();
    let unkey = (!held).then(|| rig.ptt(false));
    eng.drop_over_keyed_past_its_period(decided.into_slot_ms, past_ms);
    if let Some(why) = unkey.and_then(|unkey| slot_unkey_failure(&unkey, true)) {
        eng.halt_tx_for_failed_unkey(&why);
    }
}

/// Why a slot over must not go out on its key's answer, or `None` when it goes out: WSJT-X's rule
/// (3.0.2), on the ways a key ends here.
///
/// - **Keyed** (`RPRT 0`): WSJT-X starts the audio once the rig reports PTT on
///   (`widgets/mainwindow.cpp:12440-12448`). Goes out, unchanged.
/// - **Refused**: any other `RPRT` code (the Flex client's and the CI-V daemon's refusals are
///   `RPRT -1`), the connection closed, no rigctld, a serial PTT line that would not open. Hamlib's
///   error throws there (`Transceiver/HamlibTransceiver.cpp:277-284`, `:1292`), the rig-failure
///   handler presses Halt Tx (`mainwindow.cpp:12556-12564`), and no audio plays, since PTT never
///   came on. `Some`: nothing is played and TX halts.
/// - **Hamlib's own "the rig did not answer"** (`RPRT -5`, `-6`, `-13`, `-14`): the same throw,
///   since `error_check` excepts nothing but `RIG_OK`. `Some`, as a refusal. The other modes play
///   on these codes, with their warning (`service::KeyUp::NoAnswer`).
/// - **Nexus's PTT deadline passed** ([`crate::rig::deadline_passed`]): WSJT-X sets no deadline of
///   its own and waits on Hamlib, so a slow rig keys late and plays. `None`: goes out, as it always
///   did.
/// - **Any failure while `held`**: WSJT-X asks for PTT only on a change
///   (`Transceiver/TransceiverBase.cpp:133`), so it makes no new key. `None`: goes out on the held
///   key, as it always did.
pub(crate) fn slot_key_failure(key: &std::io::Result<()>, held: bool) -> Option<String> {
    let e = key.as_ref().err()?;
    if held || crate::rig::deadline_passed(e) {
        return None;
    }
    Some(e.to_string())
}

/// Why the unkey that ended a slot over must halt TX, or `None` when it must not: WSJT-X's rule
/// (3.0.2), on the ways an unkey ends here, as [`slot_key_failure`] is for the key. It only decides
/// the halt and what the operator is told ([`Engine::halt_tx_for_failed_unkey`]); the unkey itself
/// is never given up.
///
/// - **Unkeyed** (`RPRT 0`): `None`.
/// - **Refused**, or **Hamlib's own "the rig did not answer"** (`RPRT -5`, `-6`, `-13`, `-14`): a
///   PTT off Hamlib answers with anything but `RIG_OK` throws "… while setting PTT off"
///   (`Transceiver/HamlibTransceiver.cpp:1297-1304`, `:277-284`), the transceiver goes offline
///   (`TransceiverBase.cpp:193-206`, `:360-373`), and the main window presses Halt Tx and reports
///   the rig failure (`widgets/mainwindow.cpp:12556-12564`). `Some`.
/// - **Nexus's PTT deadline passed** ([`crate::rig::deadline_passed`]): WSJT-X sets no deadline of
///   its own and waits on Hamlib, so a slow rig unkeys late there and nothing halts. `None`.
/// - **Not a slot over's** (`slot_over` false): the voice keyer's, CW's, a tune's and every other
///   over's unkey is left as it was. `None`.
///
/// What happens to the radio after that is where Nexus does more than WSJT-X, and must go on doing
/// so. WSJT-X sends PTT off once more as it closes the rig ("try and ensure PTT isn't left set",
/// `TransceiverBase.cpp:245-247`), then nothing: closing it disconnects the rig
/// (`Configuration.cpp:5434-5452`), and reopening it sends no PTT command (`:5055`,
/// `TransceiverBase.cpp:133-137`). Here a failed unkey leaves `Rig::keyed` set, so the radio loop's
/// idle self-heal sends the unkey every tick until the radio takes it, TX halted or not.
pub(crate) fn slot_unkey_failure(unkey: &std::io::Result<()>, slot_over: bool) -> Option<String> {
    let e = unkey.as_ref().err()?;
    if !slot_over || crate::rig::deadline_passed(e) {
        return None;
    }
    Some(e.to_string())
}

/// Run one slot boundary.
///
/// At each boundary we FIRST decode the audio of the slot that just ended, THEN
/// decide whether to transmit in the new slot — the order matters so the QSO
/// auto-sequencer reacts to what we just heard (e.g. a grid reply → send a
/// report) when choosing this slot's message.
///
/// The decode is gated on **`prev_was_tx`** — whether we transmitted in the slot
/// that just ended — NOT on whether we're about to transmit now. The capture ring
/// holds one slot; if we transmitted in it, it holds our own carrier (skip), but
/// if it was a receive slot it holds the other stations and MUST be decoded even
/// when we're about to key again. (The previous logic tied the decode to the new
/// slot's TX, so calling CQ every other slot cleared each RX slot's audio without
/// ever decoding it — stations between our transmissions were never heard.)
/// `currently_tx` is the caller's `tx_until_ms.is_some()` (a TX tail crossing the
/// boundary), which also suppresses the decode.
#[allow(clippy::too_many_arguments)]
pub fn run_slot(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    rx: &mut RxRing,
    slot: u64,
    now_ms: f64,
    currently_tx: bool,
    prev_was_tx: bool,
) -> SlotAction {
    // 1. Decode the just-ended slot's RX audio first (so TX can react to it).
    //    Synchronous reference form: the live loop instead DISPATCHES the decode to
    //    the worker thread and runs step 2 only once the result lands (see
    //    `service.rs`), which keeps the exact decode→TX ordering while freeing the
    //    engine mutex + loop during the ~1–2 s decode.
    let mut rx_frame = None;
    let did_rx = if slot_wants_decode(currently_tx, prev_was_tx, rx.is_empty()) {
        let frame = rx.frame();
        eng.ingest(&frame, slot);
        rx_frame = Some(frame);
        true
    } else {
        // Own carrier (we transmitted in the just-ended slot or a TX tail is
        // crossing the boundary): the ring holds our own carrier — DROP it
        // deterministically so a fragment can't contaminate the next decode.
        if prev_was_tx || currently_tx {
            rx.clear();
        }
        false
    };

    // 2. Transmit decision for the NEW slot (now informed by the decode above).
    slot_tx_phase(
        eng,
        rig,
        backend,
        rx,
        slot,
        now_ms,
        did_rx,
        rx_frame,
        None,
        currently_tx,
    )
}

/// Whether this boundary should decode the just-ended slot's RX audio: only when
/// we did NOT transmit in it (`prev_was_tx`), no TX tail is crossing the boundary
/// (`currently_tx`), and the capture ring actually holds a period. When it holds
/// our own carrier instead, the caller clears it. Extracted so the decode gate is
/// identical between the synchronous [`run_slot`] and the async loop.
pub fn slot_wants_decode(currently_tx: bool, prev_was_tx: bool, rx_empty: bool) -> bool {
    !(prev_was_tx || currently_tx || rx_empty)
}

/// The transmit half of a slot boundary (step 2 of [`run_slot`]): make and, if due,
/// key the transmission for `slot`. Called with the decode of the just-ended slot
/// ALREADY folded in — either inline (synchronous [`run_slot`]) or via the worker
/// result (the live loop) — so the auto-sequencer reacts to what we just heard. The
/// `did_rx` / `rx_frame` describe that decode for the caller's reporting + WAV save.
#[allow(clippy::too_many_arguments)]
pub fn slot_tx_phase(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    rx: &mut RxRing,
    slot: u64,
    now_ms: f64,
    did_rx: bool,
    rx_frame: Option<Vec<f32>>,
    // A waveform already built by the caller, or `None` to build it here.
    //
    // Building takes `MODEM_LOCK` (every Fortran-backed mode; FT8 and FT4 no longer,
    // `modes::wsjt77`), which a running decode holds for the length of
    // that decode. Doing it here means doing it with the ENGINE mutex held, which
    // is what froze the UI: every Tauri snapshot and command queued behind a
    // decode. The radio loop therefore plans, RELEASES the engine, builds, and
    // hands the result in through this argument. Timing on the air is unchanged —
    // the same wait happens in the same place, just without the engine held.
    prebuilt: Option<Vec<Vec<f32>>>,
    // The loop already holds the transmitter (an over's PTT hold, or the operator's PTT): a key
    // that fails then is not a new one refused ([`key_slot_transmitter`]).
    held: bool,
) -> SlotAction {
    // JS8CALL'S LATE START: a JS8 frame decided at or past it keys nothing and is not taken from the
    // queue, so it goes out in the next period, as JS8Call's waits (`js8_slot::past_late_start`). A
    // frame built beforehand, which no path hands in that late, goes with its dial shift.
    if crate::js8_slot::past_late_start(eng, slot, now_ms) {
        if prebuilt.is_some() {
            let _ = eng.take_tx_dial_shift();
        }
        return SlotAction {
            tx_until_ms: None,
            audio_until_ms: None,
            did_rx,
            rx_frame,
            tx_this_slot: false,
            fake_it_restore: None,
            fake_it_tx_dial: None,
            rig_split_engaged: false,
        };
    }
    // ⭐ WSJT-X'S LATEST KEY, FOR A BOUNDARY WHOSE KEY COMES LATE (a decode that drained late, the
    // wait for a busy decoder, CAT in the boundary tick): an FT8/FT4 over keys only while its slot is
    // under 75 % of the period ([`wsjtx_may_key_at`]), on the clock as it is at this decision, not as
    // the tick read it. Decided before the over is planned, so an over too late to key is not planned
    // either: nothing reaches the sequencer, ALL.TXT or the band feed, and the next own slot plans it
    // again, as WSJT-X's message waits for its next slot. An over planned and built before a wait
    // (`prebuilt`, the busy decoder's) is dropped with its dial shift, as `commit_tx` drops one whose
    // slot has gone. Such a boundary used to key as long as one of its tones was still ahead. The
    // decision is kept: the key's own lateness is measured from it (below).
    let decided = wsjtx_tx_window_ms(eng.tier()).map(|_| {
        let period_ms = eng.active_slot_secs() * 1000.0;
        let reread_ms = now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64;
        KeyDecision::now(reread_ms, now_ms, period_ms)
    });
    let past_latest_key =
        decided.is_some_and(|d| !wsjtx_may_key_at(d.into_slot_ms, eng.active_slot_secs() * 1000.0));
    let waves = if eng.poll_remote_transmit(std::time::Instant::now()) {
        Vec::new()
    } else if past_latest_key {
        if prebuilt.is_some() {
            let _ = eng.take_tx_dial_shift();
        }
        Vec::new()
    } else {
        match prebuilt {
            Some(w) => w,
            None => eng.poll_tx(slot),
        }
    };
    if !waves.is_empty() {
        let period_ms = eng.active_slot_secs() * 1000.0;
        // AN FT8/FT4 OVER WHOSE TONES ARE ALL BEHIND THE SLOT CLOCK HAS NOTHING TO SEND (a boundary
        // keyed very late: past 13.14 s of FT8, 5.54 s of FT4). Decided in time (above), it keys,
        // and [`play_keyed_over`] holds it silent to the window's end, as WSJT-X's GUI tick keys an
        // over at its decision (`widgets/mainwindow.cpp:5126-5176`) and holds it, however long its
        // build took after the decision: an FT4 boundary decided 5.54-5.625 s in, or one whose
        // build, waiting on the modem or on a slow computer, ran past its latest key. That one used
        // to key nothing. Only an over with nothing left to hold either keys nothing, its clock past
        // WSJT-X's PTT off for the slot its tick decided for (13.84 s of FT8, 6.24 s of FT4), as
        // `commit_tx` keys nothing once its slot has gone, and its dial shift goes with it. The
        // clock is read here as it is now, from the start of the tick's slot, with no fallback to
        // the tick's clock (unlike [`into_tick_slot_ms`]): a re-read a period or more on, a clock
        // step or a stall alike, is past it and keys nothing, as before.
        if let Some(window_ms) = wsjtx_tx_window_ms(eng.tier()) {
            let now_wall = now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64;
            let at_ms = slot_clock_at(now_wall.max(now_ms), now_ms, period_ms);
            let slot_start_ms = (now_ms / period_ms).floor() * period_ms;
            let hold_ends_ms = wsjtx_over_end(slot_start_ms, window_ms, period_ms).ptt_off_ms;
            if !tones_after(&waves, slot_clock_skip(at_ms, period_ms))
                && now_wall.max(now_ms) >= hold_ends_ms
            {
                let _ = eng.take_tx_dial_shift();
                return SlotAction {
                    tx_until_ms: None,
                    audio_until_ms: None,
                    did_rx,
                    rx_frame,
                    tx_this_slot: false,
                    fake_it_restore: None,
                    fake_it_tx_dial: None,
                    rig_split_engaged: false,
                };
            }
        }
        // Split Operation: move the TX dial (if the engine reduced the audio)
        // BEFORE the carrier keys.
        let split = apply_tx_dial_shift(eng, rig);
        if let Some(why) = &split.refused {
            // The radio did not take the TX dial: no key, TX halted, and the split put back as
            // after any over (the native loop does it on its next idle tick).
            eng.halt_tx_for_refused_split(why);
        }
        if split.refused.is_some() || !key_slot_transmitter(eng, rig, backend, held) {
            // Preserve split cleanup even when permission disappears during
            // CAT preparation, or the radio refuses the key. The native loop restores it on its
            // next idle tick.
            return SlotAction {
                tx_until_ms: None,
                audio_until_ms: None,
                did_rx,
                rx_frame,
                tx_this_slot: false,
                fake_it_restore: split.fake_it_restore,
                fake_it_tx_dial: split.fake_it_tx_dial,
                rig_split_engaged: split.rig_split_engaged,
            };
        }
        // ⭐ AN FT8/FT4 KEY THE RADIO ANSWERED PAST THE END OF ITS SLOT IS NOT PLAYED: the radio is
        // unkeyed at once and only that over is dropped, TX left on
        // ([`unkey_a_key_past_its_period`]). Split Operation's CAT and the key itself come after
        // every slot check above, and on a slow link they took the carrier into the other station's
        // period, where the rest of the over went out on their slot clock. The split cleanup is
        // kept, as for a refused key. A key in its period ends a run of dropped overs.
        if let Some((d, past_ms)) =
            decided.and_then(|d| d.past_its_period(period_ms).map(|past_ms| (d, past_ms)))
        {
            unkey_a_key_past_its_period(eng, rig, backend, held, d, past_ms);
            return SlotAction {
                tx_until_ms: None,
                audio_until_ms: None,
                did_rx,
                rx_frame,
                tx_this_slot: false,
                fake_it_restore: split.fake_it_restore,
                fake_it_tx_dial: split.fake_it_tx_dial,
                rig_split_engaged: split.rig_split_engaged,
            };
        }
        if decided.is_some() {
            eng.note_over_keyed_in_its_period();
        }
        // ⏱ THE PTT-HOLD DEADLINE IS MEASURED FROM HERE — after the carrier is up —
        // not from the caller's `now_ms`. That was bound at the TOP of the radio-loop
        // tick, and the same tick then runs BLOCKING CAT before reaching this key: the
        // retune block (its `can_retune` gate is true at a boundary, `tx_until_ms`
        // being None), `ensure_commanded`, the split shift above, and the keying
        // round-trip itself — each able to spend a full CAT deadline (700 ms, 2500 ms
        // on a slow transport). A deadline built on `now_ms` is therefore measured from
        // an instant already past, and unkeys EARLY by exactly the CAT time, cutting
        // the tail off our own over. MSK144 is where it shows: 14.688 s of audio in a
        // 15 s slot (period − 0.25 s truncated to whole 72 ms frames) leaves ~0.31 s of
        // slack — less than one slow CAT exchange. FT8's 13.14 s leaves ~1.86 s, so
        // every other tier absorbs the same stall as a merely shortened tail.
        //
        // ⚠️ STEER THE RE-READ. `now_ms` is on the UTC-STEERED clock (`service.rs`
        // SUBTRACTS the measured PC-clock-vs-UTC offset so TX keys on the true UTC
        // grid). A bare `now_unix_ms()` here would be on a different timebase and shift
        // the deadline by the whole offset — worst on exactly the badly-synced machines
        // the steering exists to rescue. Same expression as the busy-worker branch's
        // post-build re-read in `service.rs`.
        //
        // Floored at `now_ms`: a backwards clock step (an NTP correction landing mid-
        // tick) must never make this hold SHORTER than the caller's basis — that is the
        // failure this whole re-read exists to prevent.
        //
        // FT8 and FT4 take the same re-read for a different job: not their deadline, which is
        // WSJT-X's window on the slot clock, but how far into the slot the carrier came up, so
        // their tones are laid on the clock as WSJT-X's Modulator lays them ([`play_keyed_over`]).
        let keyed_ms = (now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64).max(now_ms);
        // JS8: a key the radio answered once the frame's last tone had gone sends nothing and is
        // unkeyed at once unless the loop holds the transmitter; TX stays on
        // (`js8_slot::late_key`). Every other frame is laid on the slot clock, keyed in its TX delay
        // or late in its period (`js8_slot::play_frame`).
        if let Some((into_ms, limit_ms)) = crate::js8_slot::late_key(eng, keyed_ms, slot) {
            crate::js8_slot::unkey_a_late_key(eng, rig, backend, held, into_ms, limit_ms);
            return SlotAction {
                tx_until_ms: None,
                audio_until_ms: None,
                did_rx,
                rx_frame,
                tx_this_slot: false,
                fake_it_restore: split.fake_it_restore,
                fake_it_tx_dial: split.fake_it_tx_dial,
                rig_split_engaged: split.rig_split_engaged,
            };
        }
        let over = if eng.tier() == tempo_app::dto::Tier::Js8 {
            crate::js8_slot::play_frame(eng, backend, &waves, keyed_ms, slot)
        } else {
            play_keyed_over(eng, backend, &waves, keyed_ms, now_ms)
        };
        rx.clear(); // our just-started carrier must not be decoded next boundary
                    // The bound the re-read above deliberately does NOT provide. Measuring the
                    // deadline from the key (rather than the tick's stale clock) stopped us
                    // unkeying early; it also means keying latency lands wholly on the far end of
                    // the over, and nothing was stopping that from running past the boundary. See
                    // [`tx_deadline_ms`] — this is WSJT-X's clamp, and FT1 is where the absence
                    // showed: its buffer FILLS its period, so it is clamped on every over.
        SlotAction {
            tx_until_ms: Some(over.tx_until_ms),
            audio_until_ms: over.audio_until_ms,
            did_rx,
            rx_frame,
            tx_this_slot: true,
            fake_it_restore: split.fake_it_restore,
            fake_it_tx_dial: split.fake_it_tx_dial,
            rig_split_engaged: split.rig_split_engaged,
        }
    } else {
        // Receive slot: keep the rolling capture window (no clear) so the next
        // boundary decodes this slot's audio.
        SlotAction {
            tx_until_ms: None,
            audio_until_ms: None,
            did_rx,
            rx_frame,
            tx_this_slot: false,
            fake_it_restore: None,
            fake_it_tx_dial: None,
            rig_split_engaged: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;
    use tempo_app::engine::{run_decode_job, DecodeApplied, DecodePass};

    #[test]
    fn remote_presence_keying_rechecks_authority_before_and_after_ptt_io() {
        use crate::rig::remote_tests::{retuning_peer, writes};
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        use tempo_app::remote_control::transmit::TransmitAuthority;
        for scene in ["valid", "beforeKey", "duringKey"] {
            let authority = Arc::new(TransmitAuthority::default());
            let revoke = authority.clone();
            let peer = retuning_peer(14_074_000, "PKTUSB", move |line, state| {
                if line == "T 1" {
                    state.keyed = true;
                    if scene == "duringKey" {
                        revoke.revoke();
                    }
                    Some("RPRT 0\n".into())
                } else {
                    None
                }
            });
            let mut e = Engine::new("KD9TAW", "EN52", 0);
            // Salted by pid AND scene: concurrent `cargo test` runs from sibling worktrees
            // share one `$TMPDIR`, so a fixed filename here is one settings store shared by
            // every run. The scene is in the name too, so the three scenes cannot carry
            // persisted state into one another — the engine is rebuilt each iteration, so
            // a shared store would be the only thing coupling them, and that coupling would
            // be invisible.
            e.configure_remote_settings_store(
                std::env::temp_dir()
                    .join(format!("nexus-ft-key-{}-{scene}.json", std::process::id())),
            );
            e.set_tx_enabled(false);
            e.take_immediate_retune();
            // A streamed operator's Call CQ: the native verb, under the session's presence.
            // Presence that cannot lapse mid-test: revocation, not the clock, is this case's
            // subject — `authority.revoke()` drives all three scenes.
            e.start_cq(None).unwrap();
            let now = Instant::now();
            assert!(e.hold_remote_presence(
                authority
                    .permit(now + Duration::from_secs(24 * 60 * 60))
                    .unwrap(),
                now,
            ));
            let mut rig = Rig::rigctld(&peer.address);
            let mut backend = MockBackend::new();
            let mut rx = RxRing::with_capacity(120);
            if scene == "beforeKey" {
                authority.revoke();
            }
            // The waveform was already committed before the asynchronous revoke.
            let action = slot_tx_phase(
                &mut e,
                &mut rig,
                &mut backend,
                &mut rx,
                0,
                0.0,
                false,
                None,
                Some(vec![vec![0.25; 120]]),
                false,
            );
            if scene == "valid" {
                assert!(action.tx_this_slot);
                assert_eq!(backend.played.len(), 120);
                assert_eq!(writes(&peer), ["T 1"]);
            } else {
                assert!(!action.tx_this_slot, "{scene}");
                assert!(backend.played.is_empty(), "{scene}");
                assert!(!rig.keyed, "{scene}");
                assert!(!e.tx_enabled(), "{scene}");
                assert!(e.take_slot_tx_abort(), "{scene}");
                if scene == "duringKey" {
                    assert_eq!(writes(&peer), ["T 1", "T 0"]);
                } else {
                    assert!(writes(&peer).is_empty());
                }
            }
        }
    }

    /// The clock instant a keyed [`SlotAction`]'s PTT-hold deadline was built from:
    /// strip the played audio's duration and the fixed tail back off. That basis is
    /// the whole subject of the three tests below.
    fn deadline_basis_ms(act: &SlotAction, backend: &MockBackend) -> f64 {
        let audio_ms = backend.played.len() as f64 / tempo_fast::SAMPLE_RATE as f64 * 1000.0;
        act.tx_until_ms.expect("the over keyed") - audio_ms - TX_TAIL_MS
    }

    /// "Now" on the same UTC-steered timebase the radio loop hands in as `now_ms`
    /// (`service.rs` subtracts the measured PC-clock-vs-UTC offset from the system
    /// clock so TX keys on the true UTC grid).
    fn steered_now_ms(eng: &Engine) -> f64 {
        now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64
    }

    /// An engine armed with one over to send, on the default (FT8) tier.
    fn armed_engine() -> Engine {
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx)
        eng.broadcast("CQ TEST W9XYZ EN37");
        align_to_slot_start(&mut eng, 0);
        eng
    }

    /// [`armed_engine`] on `tier`.
    ///
    /// ⚠️ FT2 IS WHERE THE KEY-RELATIVE DEADLINE IS TESTED NOW. FT8 and FT4 end on WSJT-X's window
    /// on the slot clock (`wsjtx_over_end`, pinned by the `wsjtx_*` and `an_ft8_overs_end_*` tests
    /// below) and lay their tones on that clock (`play_keyed_over`), so neither the deadline's basis
    /// nor a fake over's length is the key's any more there. Every other slot tier still holds PTT
    /// from the key ([`tx_deadline_ms`]), and FT2's 980 ms of slack keeps an on-time or a 300 ms-late
    /// key clear of its clamp, as FT8's 1.86 s did.
    fn armed_engine_on(tier: tempo_app::dto::Tier) -> Engine {
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tier);
        // The tier change's context halt asks the loop to cut, which the loop's next tick takes.
        let _ = eng.take_slot_tx_abort();
        eng.set_tx_enabled(true);
        eng.broadcast("CQ TEST W9XYZ EN37");
        align_to_slot_start(&mut eng, 0);
        eng
    }

    /// Steer the engine's clock so an over keyed *now* starts just after a slot
    /// boundary, plus `skew_ms` of PC-clock error rounded to whole periods.
    ///
    /// ⚠️ WHY THE BASIS TESTS NEED THIS. Their deadline is built from a re-read of the
    /// real clock, so without steering, the over's PHASE within its slot is whatever
    /// time the suite happened to run at. Since [`tx_deadline_ms`] bounds the deadline
    /// at the boundary, an unaligned run truncates it at some phases and not others —
    /// which would turn "the basis is the key, not the stale tick" into a coin flip
    /// that passes ~14% of the time (FT8 leaves 2.11 s of slack in 15 s). Aligning
    /// keeps the clamp a no-op so those assertions still measure what they were
    /// written to measure. `offset ≡ now (mod period)` is what puts the steered clock
    /// on a boundary; adding whole periods preserves the congruence, which is how a
    /// large skew and a known phase coexist.
    ///
    /// ⚠️ The resulting offset must stay inside `clocksync::MAX_STEER_MS` (60 s):
    /// guard 3 refuses to steer by anything larger, so a bigger skew here would
    /// silently give the engine NO offset and the caller would be testing the
    /// unsteered path while believing otherwise. Asserted, not assumed.
    fn align_to_slot_start(eng: &mut Engine, skew_ms: i64) {
        let period_ms = eng.active_slot_secs() * 1000.0;
        let phase = now_unix_ms().rem_euclid(period_ms);
        let whole = (skew_ms as f64 / period_ms).round() * period_ms;
        let offset = (phase + whole) as i64;
        eng.set_clock_offset_ms(Some(offset));
        assert_eq!(
            eng.clock_offset_ms(),
            Some(offset),
            "the skew must be inside guard 3's ceiling or this helper aligns nothing"
        );
    }

    /// A throwaway rigctld that answers every command with `RPRT 0` — but only
    /// after `delay_ms`. Models the real hazard: a CAT link whose keying round-trip
    /// eats hundreds of ms (the `Rig` PTT deadline allows up to 700).
    fn slow_rigctld(delay_ms: u64) -> String {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            let mut sock = match listener.accept() {
                Ok((s, _)) => s,
                Err(_) => return,
            };
            let mut buf = [0u8; 256];
            loop {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                if sock.write_all(b"RPRT 0\n").is_err() {
                    return;
                }
            }
        });
        addr
    }

    // ── WSJT-X's slot clamp ──────────────────────────────────────────────────
    // These drive [`tx_deadline_ms`] directly rather than through `slot_tx_phase`,
    // because the deadline there is built from a re-read of the REAL clock: the
    // phase of the over within its slot would be whatever time the suite ran at,
    // and a bound that only bites at some phases would be a coin-flip test. The
    // arithmetic is the whole of the fix, so it is tested where it is decidable.

    /// ⚠️ WHAT PRODUCTION ACTUALLY PASSES: `w.len()` of the FIXED NMAX = 48000-sample buffer
    /// `tempo_fast::gen_wave` returns — 4.0000 s, tones PLUS trailing zeros. It is NOT the
    /// 3.5357 s tone length; using that here was the error behind the "214 ms of slack" claim,
    /// and it made `an_on_time_ft1_over_keeps_its_whole_tail` assert a case that cannot occur.
    const FT1_AUDIO_MS: f64 = 48_000.0 / 12_000.0 * 1000.0;
    /// The tone length, for the tests that are about where the SIGNAL ends rather than the buffer.
    const FT1_TONES_MS: f64 = 99.0 * (3000.0 / 7.0) / 12_000.0 * 1000.0;
    const FT1_PERIOD_MS: f64 = 4_000.0;
    /// FT1's lead-in, shifted WITHIN the fixed buffer — mirrors `FT1_LEAD_IN_SECS`
    /// (crates/modes/src/mode.rs). ⚠️ MOVES WITH IT: the tail this file asserts is
    /// `464 ms - lead`, so a change there without a change here silently stops testing the real
    /// geometry. 300 ms since 2026-08-05; the trade is measured in the ft1_lead_sweep test.
    const FT1_LEAD_MS: f64 = 300.0;

    #[test]
    fn an_ft1_over_keyed_late_still_ends_at_the_boundary() {
        // THE DEFECT. FT1 leaves 214 ms of slack (3535.7 ms of audio + a 250 ms tail
        // in a 4 s slot), which is less than one blocking CAT exchange. Keyed 500 ms
        // into the slot, the unclamped deadline runs 286 ms into the NEXT period —
        // transmitting over the slot we are supposed to be receiving in.
        let slot_start = 1_000_000.0 * FT1_PERIOD_MS; // a real boundary
        let keyed = slot_start + 500.0;
        let unclamped = keyed + FT1_AUDIO_MS + TX_TAIL_MS;
        assert!(
            unclamped > slot_start + FT1_PERIOD_MS,
            "the scenario must actually overrun, else this test proves nothing"
        );
        let got = tx_deadline_ms(keyed, FT1_AUDIO_MS, FT1_PERIOD_MS);
        assert_eq!(got, slot_start + FT1_PERIOD_MS, "must end AT the boundary");
        assert!(got < unclamped, "and that is earlier than it used to unkey");
    }

    #[test]
    fn even_a_perfectly_on_time_ft1_over_is_clamped_and_keeps_only_64ms_of_tail() {
        // ⚠️ THIS TEST ASSERTED THE OPPOSITE AND WAS WRONG. It claimed FT1 "fits with 214 ms to
        // spare and nothing is taken off the tail" — a case that CANNOT OCCUR, because it was
        // computed from FT1's 3.5357 s TONE length while production sizes the hold from the
        // FIXED 4.0000 s buffer. There is no unclamped FT1 over at any phase.
        //
        // What is actually true, and what an operator should know: FT1's buffer fills its whole
        // period, so the clamp fires on every over and the effective post-tone tail is a fixed
        // 64.2 ms. That is the margin protecting the end of the signal from soundcard latency,
        // and it is thin — see the hazard note on `tx_deadline_ms`.
        let slot_start = 1_000_000.0 * FT1_PERIOD_MS;
        let got = tx_deadline_ms(slot_start, FT1_AUDIO_MS, FT1_PERIOD_MS);
        assert_eq!(
            got,
            slot_start + FT1_PERIOD_MS,
            "an on-time FT1 over is STILL clamped — 4.000 s of buffer + 250 ms tail in a 4.000 s \
             period leaves -250 ms, not the +214 ms this test used to claim"
        );
        // The tail that actually survives, measured from where the TONES end.
        let tone_end = slot_start + FT1_LEAD_MS + FT1_TONES_MS;
        let tail_ms = got - tone_end;
        assert!(
            (tail_ms - 164.2).abs() < 0.5,
            "FT1's real post-tone tail is ~164.2 ms at the 300 ms lead, got {tail_ms:.1} — if \
             this moved, so did the margin protecting the end of the signal from soundcard \
             output latency, which is the whole reason the lead was shortened"
        );
    }

    #[test]
    fn no_tier_can_key_past_its_own_boundary_at_any_phase() {
        // The bound is upstream's and applies to every mode, not only the one that
        // exposed it. FT8 12.64 s / 15 s and FT4 5.04 s / 7.5 s have enough slack to
        // absorb an ordinary stall, but a pathological one must not put THEM into the
        // next period either — the clamp is what makes that unrepresentable.
        for (audio_ms, period_ms, name) in [
            (FT1_AUDIO_MS, FT1_PERIOD_MS, "FT1"),
            (12_640.0, 15_000.0, "FT8"),
            (5_040.0, 7_500.0, "FT4"),
            (14_688.0, 15_000.0, "MSK144-15"),
            // FT2: 2.52 s of tones from the slot start in a 3.75 s period. The SHORTEST
            // period here, but not the tightest fit — 3020 + 250 ms of tail leaves
            // ~980 ms of slack, more than MSK144-15's ~312 ms and unlike FT1's −250 ms
            // it does not clamp on an on-time over. Swept anyway: the clamp is what
            // makes a pathological stall unrepresentable rather than merely unlikely.
            (2_520.0, 3_750.0, "FT2"),
            // The BEACON modes (#101a): now that WSPR/FST4W are exempt from the
            // wall-clock TX watchdog (operator-approved, matching WSJT-X), this clamp
            // is the ONLY in-flight bound on a beacon over — so it is swept explicitly.
            // WSPR: 110.6 s of tones in a 120 s period; FST4W-1800: ~1770 s in 1800 s.
            (110_600.0, 120_000.0, "WSPR"),
            (1_770_000.0, 1_800_000.0, "FST4W-1800"),
        ] {
            let slot_start = 1_000.0 * period_ms;
            // Every 10 ms of phase across a whole slot.
            let mut phase = 0.0;
            while phase < period_ms {
                let keyed = slot_start + phase;
                let got = tx_deadline_ms(keyed, audio_ms, period_ms);
                assert!(
                    got <= slot_start + period_ms,
                    "{name} keyed at phase {phase} ms held PTT {:.1} ms past its boundary",
                    got - (slot_start + period_ms),
                );
                assert!(got >= keyed, "{name} produced a deadline before the key");
                phase += 10.0;
            }
        }
    }

    #[test]
    fn ft2_has_real_slack_in_the_shortest_period_in_the_app() {
        // ⭐ THE MARGIN CHECK FOR THE TIGHTEST SLOT CLOCK WE SHIP. FT2's 3.75 s
        // period is shorter than FT1's 4 s, and FT1 "carries no slack at all" — so
        // the question is whether FT2 inherits that hazard. It does NOT, and the
        // reason is the buffer: FT1's gen_wave returns a FIXED full-period 4.000 s
        // buffer, while FT2's returns exactly lead + tones and stops.
        //
        // Asserted rather than reasoned, because the failure mode is silent: a
        // negative margin does not error, it quietly clamps every over and eats the
        // tail that protects the end of the signal from soundcard output latency.
        const FT2_AUDIO_MS: f64 = 2_520.0; // 30240 samples of tones from the slot start
        const FT2_PERIOD_MS: f64 = 3_750.0;
        let slack = FT2_PERIOD_MS - (FT2_AUDIO_MS + TX_TAIL_MS);
        assert!(
            slack > 0.0,
            "FT2's over no longer fits its own period: {slack:.1} ms of slack. Do NOT widen the \
             clamp to make this pass — the geometry changed, and an over that overruns \
             transmits into the period we are supposed to be receiving in"
        );
        assert!(
            (slack - 980.0).abs() < 1.0,
            "FT2's slack moved from the 980 ms this build ships, to {slack:.1} ms"
        );
        // And the consequence that slack buys: an ON-TIME FT2 over is NOT clamped,
        // so it keeps its whole tail — unlike FT1, which is clamped at every phase.
        let slot_start = 1_000_000.0 * FT2_PERIOD_MS;
        let got = tx_deadline_ms(slot_start, FT2_AUDIO_MS, FT2_PERIOD_MS);
        assert_eq!(
            got,
            slot_start + FT2_AUDIO_MS + TX_TAIL_MS,
            "an on-time FT2 over must reach its natural end, not the boundary clamp"
        );
    }

    #[test]
    fn an_over_keyed_past_its_boundary_releases_at_once() {
        // Degenerate, but it must not produce a deadline in the past: a hold the loop
        // reads as already expired is fine, a negative one is not.
        let slot_start = 1_000_000.0 * FT1_PERIOD_MS;
        let keyed = slot_start + FT1_PERIOD_MS; // exactly the next boundary
        let got = tx_deadline_ms(keyed, FT1_AUDIO_MS, FT1_PERIOD_MS);
        assert!(got >= keyed, "never a deadline before the key");
    }

    #[test]
    fn a_nonsense_period_falls_back_to_the_unbounded_deadline() {
        // A zero/NaN period must not silently unkey us instantly — better to hold as
        // we always did than to cut every over on a bad reading.
        let keyed = 4_000_000.0;
        let natural = keyed + FT1_AUDIO_MS + TX_TAIL_MS;
        assert_eq!(tx_deadline_ms(keyed, FT1_AUDIO_MS, 0.0), natural);
        assert_eq!(tx_deadline_ms(keyed, FT1_AUDIO_MS, f64::NAN), natural);
    }

    // ── WSJT-X's over window, and the tones on the slot clock (FT8 and FT4) ──────────────────────
    // Upstream master, checked line by line: the window is `txDuration` from the slot start
    // (helper_functions.cpp:6-7; widgets/mainwindow.cpp:5008-5015, 5055), the GUI tick past it stops
    // the audio (:5180, :5530 → stopTx :5743-5753), PTT drops 200 ms later (ptt0Timer → stopTx2,
    // :5753-5760), and the Modulator lays the tones on the clock (Modulator/Modulator.cpp:56-57,
    // 85-101).

    const FT8_PERIOD_MS: f64 = 15_000.0;
    const FT4_PERIOD_MS: f64 = 7_500.0;
    /// FT8's slot-positioned wave: 0.5 s of lead-in and 79 symbols of 1920 samples (12.64 s).
    const FT8_WAVE: usize = 6_000 + 79 * 1_920;

    #[test]
    fn an_on_time_ft8_or_ft4_over_ends_where_wsjtx_ends_it() {
        // ⭐ WSJT-X's times, by value. FT8: audio stops 13.64 s into the slot, PTT 13.84 s. FT4:
        // 6.04 s and 6.24 s. The over used to end at key + wave + 250 ms: FT8 13.39 s (0.45 s of
        // PTT and 0.25 s of output-latency room short of WSJT-X), FT4 6.80 s (0.56 s longer).
        assert_eq!(
            wsjtx_tx_window_ms(tempo_app::dto::Tier::Ft8),
            Some(13_640.0)
        );
        assert_eq!(wsjtx_tx_window_ms(tempo_app::dto::Tier::Ft4), Some(6_040.0));
        let slot_start = 1_000.0 * FT8_PERIOD_MS;
        assert_eq!(
            wsjtx_over_end(slot_start + 20.0, 13_640.0, FT8_PERIOD_MS),
            OverEnd {
                audio_stop_ms: slot_start + 13_640.0,
                ptt_off_ms: slot_start + 13_840.0,
            }
        );
        let slot_start = 1_000.0 * FT4_PERIOD_MS;
        assert_eq!(
            wsjtx_over_end(slot_start + 20.0, 6_040.0, FT4_PERIOD_MS),
            OverEnd {
                audio_stop_ms: slot_start + 6_040.0,
                ptt_off_ms: slot_start + 6_240.0,
            }
        );
        // Every other tier keeps the deadline it had.
        for tier in [
            tempo_app::dto::Tier::Ft2,
            tempo_app::dto::Tier::Js8,
            tempo_app::dto::Tier::Q65,
            tempo_app::dto::Tier::Msk144,
            tempo_app::dto::Tier::TempoFast,
        ] {
            assert_eq!(wsjtx_tx_window_ms(tier), None, "{tier:?}");
        }
    }

    #[test]
    fn a_late_ft8_key_keeps_its_end_and_loses_its_first_symbols_on_the_slot_clock() {
        // ⭐ WSJT-X's late start (Modulator.cpp:96-100): the symbols already gone are skipped, so
        // what is sent sits on the slot grid (DT 0) and ends where an on-time over ends. Nexus played
        // the whole over from the key: every tone late by the key's lateness, the end clamped at the
        // boundary.
        let slot_start = 1_000.0 * FT8_PERIOD_MS;
        let on_time = wsjtx_over_end(slot_start + 20.0, 13_640.0, FT8_PERIOD_MS);
        for late_ms in [500.0, 2_000.0, 7_900.0] {
            let keyed = slot_start + late_ms;
            assert_eq!(
                wsjtx_over_end(keyed, 13_640.0, FT8_PERIOD_MS),
                on_time,
                "a key {late_ms} ms late moved the end of the over"
            );
            let skip = slot_clock_skip(keyed, FT8_PERIOD_MS);
            assert_eq!(skip, (late_ms * 12.0) as usize, "{late_ms} ms late");
            // What plays is the rest of the wave from there, so its last sample still leaves at
            // the slot's 13.14 s, whatever the key's lateness.
            let last_sample_at = keyed + (FT8_WAVE - skip) as f64 / 12.0;
            assert_eq!(last_sample_at, slot_start + 13_140.0, "{late_ms} ms late");
        }
    }

    #[test]
    fn an_early_start_keeps_the_rest_of_the_lead_in_as_wsjtxs_silent_frames() {
        // WSJT-X sends silence until the tones' nominal start (`m_silentFrames`,
        // Modulator.cpp:91-94). The lead-in at the head of the wave is that silence: a key 100 ms
        // into the slot skips 100 ms of it, and the tones still start at 0.5 s.
        let slot_start = 1_000.0 * FT4_PERIOD_MS;
        let skip = slot_clock_skip(slot_start + 100.0, FT4_PERIOD_MS);
        assert_eq!(skip, 1_200);
        assert_eq!(
            6_000 - skip,
            4_800,
            "0.4 s of lead-in left: the tones start at 0.5 s"
        );
        assert_eq!(
            slot_clock_skip(slot_start, FT4_PERIOD_MS),
            0,
            "on the boundary: all of it"
        );
    }

    #[test]
    fn wsjtxs_window_never_holds_ptt_past_the_boundary_at_any_phase() {
        // The period clamp, kept: at every phase of every key, PTT drops inside the slot it was
        // keyed in and never before the key, and the audio stops no later than PTT.
        for (window_ms, period_ms, name) in [
            (13_640.0, FT8_PERIOD_MS, "FT8"),
            (6_040.0, FT4_PERIOD_MS, "FT4"),
            // A window past its period, clamped as WSJT-X clamps `tx2`: PTT at the boundary.
            (16_000.0, FT8_PERIOD_MS, "a window past the period"),
        ] {
            let slot_start = 1_000.0 * period_ms;
            let mut phase = 0.0;
            while phase < period_ms {
                let keyed = slot_start + phase;
                let end = wsjtx_over_end(keyed, window_ms, period_ms);
                assert!(
                    end.ptt_off_ms <= slot_start + period_ms,
                    "{name}: keyed at {phase} ms, PTT {:.1} ms past the boundary",
                    end.ptt_off_ms - (slot_start + period_ms)
                );
                assert!(end.ptt_off_ms >= keyed, "{name}: PTT off before the key");
                assert!(
                    end.audio_stop_ms <= end.ptt_off_ms && end.audio_stop_ms >= keyed,
                    "{name}: keyed at {phase} ms, {end:?}"
                );
                phase += 10.0;
            }
        }
        let slot_start = 1_000.0 * FT8_PERIOD_MS;
        assert_eq!(
            wsjtx_over_end(slot_start, 16_000.0, FT8_PERIOD_MS),
            OverEnd {
                audio_stop_ms: slot_start + FT8_PERIOD_MS,
                ptt_off_ms: slot_start + FT8_PERIOD_MS,
            },
            "the window is clamped to the period, and PTT with it"
        );
    }

    /// An FT8 over keyed through the TX phase on a clock steered `skew_ms` (whole periods), its key
    /// taking `key_ms`, the tick's clock `stale_ms` old: the action, the backend, the steered slot's
    /// start, and the steered clock read before and after the call.
    fn ft8_over_on_the_steered_clock(
        skew_ms: i64,
        key_ms: u64,
        stale_ms: f64,
    ) -> (SlotAction, MockBackend, f64, f64, f64) {
        let mut eng = armed_engine();
        align_to_slot_start(&mut eng, skew_ms);
        // The stale tick read its clock inside the slot it keys, as the loop's always does (it keys
        // a slot only from a tick in it): the steered clock goes `stale_ms` into the slot first.
        // From a tick in the slot before, the key would be in the next slot, which WSJT-X's latest
        // key refuses (`into_tick_slot_ms`).
        let offset = eng.clock_offset_ms().unwrap_or(0) - stale_ms as i64;
        eng.set_clock_offset_ms(Some(offset));
        assert_eq!(eng.clock_offset_ms(), Some(offset), "premise: steered");
        let mut rig = Rig::rigctld(&slow_rigctld(key_ms));
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        let steered = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            steered - stale_ms,
            false,
            None,
            None,
            false,
        );
        let after = steered_now_ms(&eng);
        let slot_start = (steered / FT8_PERIOD_MS).floor() * FT8_PERIOD_MS;
        (act, backend, slot_start, steered, after)
    }

    #[test]
    fn an_ft8_overs_end_is_on_the_steered_slot_clock_whatever_the_tick_or_the_key() {
        // The three ways a key-relative deadline went wrong (the basis tests above, now on FT2) no
        // longer reach FT8's end at all: it is measured from the slot, not the key or the tick. A
        // stale tick clock, a slow key and a PC clock 30 s off UTC all leave it at the steered
        // slot's 13.64 s / 13.84 s, and the audio is laid on that same clock: what played is the
        // wave from where the slot was when the carrier came up.
        //
        // ⚠️ What catches a RAW-clock re-read here is the skip, at skew 0: `align_to_slot_start`
        // steers by the clock's own phase (0..15 s), so a raw read lands that far into the slot
        // and skips that much. At 30 s the raw read is two periods off the tick's clock and
        // `slot_clock_at` falls back to the tick's (a guard for a clock step mid-tick), so that
        // row shows the window right under a large skew but could not tell the two clocks apart.
        for (skew_ms, key_ms, stale_ms, what) in [
            (0, 0, 400.0, "a tick clock 400 ms stale"),
            (0, 300, 0.0, "a key that takes 300 ms"),
            (30_000, 0, 0.0, "a PC clock 30 s ahead of UTC"),
        ] {
            let (act, backend, slot_start, steered, after) =
                ft8_over_on_the_steered_clock(skew_ms, key_ms, stale_ms);
            assert!(act.tx_this_slot, "{what}: premise: the CQ keyed");
            assert_eq!(
                (act.audio_until_ms, act.tx_until_ms),
                (Some(slot_start + 13_640.0), Some(slot_start + 13_840.0)),
                "{what}: the over does not end on WSJT-X's window on the steered slot"
            );
            let skipped = FT8_WAVE - backend.played.len();
            let lo = ((steered - slot_start) * 12.0).floor() as usize;
            let hi = ((after - slot_start) * 12.0).ceil() as usize;
            assert!(
                (lo..=hi).contains(&skipped),
                "{what}: skipped {skipped} samples, expected the key's place in the slot, \
                 {lo}..={hi}"
            );
            if key_ms > 0 {
                assert!(
                    skipped >= (key_ms * 12) as usize,
                    "{what}: the {key_ms} ms the key took was not skipped ({skipped})"
                );
            }
        }
    }

    #[test]
    fn a_boundary_with_no_tone_left_keys_under_its_latest_key_and_is_held_to_the_window() {
        // A boundary keyed after the last tone's time (13.14 s of FT8) has nothing left to send. Past
        // WSJT-X's latest key (11.25 s) it keys nothing, and its dial shift goes with it, as
        // `commit_tx` keys nothing once its slot has gone. It used to key and play the head of the
        // message for the rest of the slot.
        //
        // FT4's tones end at 5.54 s, under its latest key of 5.625 s. A boundary 5.58 s in keys there,
        // as WSJT-X's GUI tick keys it (`widgets/mainwindow.cpp:5126-5176`): nothing plays, and PTT is
        // held to the window's end, the audio stop at 6.04 s and PTT off at 6.24 s. It used to key
        // nothing. 5.50 s, with 40 ms of tones still ahead, is the control.
        let mut eng = armed_engine();
        let late = steered_now_ms(&eng) + 13_200.0;
        let (addr, log, _) = keying_rigctld("RPRT 0", 0);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            late,
            false,
            None,
            Some(vec![vec![0.25; FT8_WAVE]]),
            false,
        );
        let sent = log.lock().unwrap().clone();
        assert!(sent.is_empty(), "a command went on the wire: {sent:?}");
        assert!(!act.tx_this_slot && act.tx_until_ms.is_none());
        assert!(backend.played.is_empty());
        assert!(eng.tx_enabled(), "a missed over is not a halt");

        // FT4 5.58 s: under its latest key, past its tones.
        let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft4);
        let slot_start = steered_slot_start(&eng);
        let (act, backend, sent) = tx_phase_at(&mut eng, 5_580.0, Some(vec![ft4_stand_in()]));
        assert_eq!(sent, ["T 1"], "FT4 5.58 s: not keyed");
        assert!(
            act.tx_this_slot && backend.played.is_empty(),
            "FT4 5.58 s: {} samples played",
            backend.played.len()
        );
        assert_eq!(
            (act.audio_until_ms, act.tx_until_ms),
            (Some(slot_start + 6_040.0), Some(slot_start + 6_240.0)),
            "FT4 5.58 s: not held to WSJT-X's window"
        );
        assert!(eng.tx_enabled(), "FT4: TX went off");

        // The control: FT4 5.50 s, 40 ms of tones still ahead of the clock, and it keys.
        let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft4);
        let (act, backend, sent) = tx_phase_at(&mut eng, 5_500.0, Some(vec![ft4_stand_in()]));
        assert_eq!(sent, ["T 1"], "control: the key");
        assert!(act.tx_this_slot && !backend.played.is_empty(), "control");
    }

    // ── WSJT-X's latest key: 75 % of the period (FT8 and FT4) ──────────────────────────────────
    // Upstream master, `widgets/mainwindow.cpp:5114` and `:5126`: the GUI tick asserts PTT only while
    // `fTR`, the slot's place as a fraction of its period, is under 0.75 ("Allow late starts"), and
    // the Modulator then skips the symbols already gone. A late key past it waits for the next slot.

    /// FT4's slot-positioned wave: 0.5 s of lead-in, 105 symbols of 576 samples of a constant tone (to
    /// 5.54 s), and the 1.008 s of silence the real one carries after them.
    fn ft4_stand_in() -> Vec<f32> {
        let mut wave = vec![0.0; 6_000];
        wave.resize(6_000 + 105 * 576, 0.25);
        wave.resize(78_576, 0.0);
        wave
    }

    /// The start of the slot `eng`'s steered clock is in.
    fn steered_slot_start(eng: &Engine) -> f64 {
        let period_ms = eng.active_slot_secs() * 1000.0;
        (steered_now_ms(eng) / period_ms).floor() * period_ms
    }

    /// The TX phase of the slot `eng`'s steered clock is in, from a tick whose clock reads `into_ms`
    /// into it, on a rigctld that keys: the action, what played, and every line the rigctld got.
    /// The tick's clock is ahead of the wall clock, so the decision is exactly `into_ms` in.
    fn tx_phase_at(
        eng: &mut Engine,
        into_ms: f64,
        prebuilt: Option<Vec<Vec<f32>>>,
    ) -> (SlotAction, MockBackend, Vec<String>) {
        let slot_start = steered_slot_start(eng);
        let (addr, log, _) = keying_rigctld("RPRT 0", 0);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        let act = slot_tx_phase(
            eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            slot_start + into_ms,
            false,
            None,
            prebuilt,
            false,
        );
        let sent = log.lock().unwrap().clone();
        (act, backend, sent)
    }

    #[test]
    fn wsjtxs_latest_key_is_its_own_arithmetic_at_every_ms() {
        // ⭐ BY VALUE AGAINST WSJT-X'S OWN EXPRESSION, at every millisecond of an FT8 and an FT4 slot:
        // `fTR=float((ms%int(1000.0*m_TRperiod)))/int(1000.0*m_TRperiod)`, keyed while `fTR < 0.75`.
        for period_ms in [FT8_PERIOD_MS, FT4_PERIOD_MS] {
            let p = period_ms as i64;
            for ms in 0..p {
                let f_tr = ((ms % p) as f32 / p as f32) as f64;
                assert_eq!(
                    wsjtx_may_key_at(ms as f64, period_ms),
                    f_tr < 0.75,
                    "{ms} ms into a {p} ms slot"
                );
            }
        }
        // FT8 keys through 11.249 s and not at 11.250 s; FT4 through 5.624 s and not at 5.625 s.
        assert!(wsjtx_may_key_at(11_249.0, FT8_PERIOD_MS));
        assert!(!wsjtx_may_key_at(11_250.0, FT8_PERIOD_MS));
        assert!(wsjtx_may_key_at(5_624.0, FT4_PERIOD_MS));
        assert!(!wsjtx_may_key_at(5_625.0, FT4_PERIOD_MS));
    }

    #[test]
    fn the_latest_key_reads_the_slot_where_the_decision_is_not_where_the_tick_read_it() {
        let slot_start = 1_000.0 * FT8_PERIOD_MS;
        let tick = slot_start + 11_000.0;
        // Read again 300 ms after the tick read it (CAT earlier in the tick): 11.3 s, too late.
        assert_eq!(
            into_tick_slot_ms(tick + 300.0, tick, FT8_PERIOD_MS),
            11_300.0
        );
        // A loop on a clock of its own, ahead of the wall clock (the loop's tests): the tick's.
        assert_eq!(
            into_tick_slot_ms(tick - 40_000.0, tick, FT8_PERIOD_MS),
            11_000.0
        );
        // A wall-clock step of a period or more since the tick read it: the tick's, as the tones'
        // placement takes it (`slot_clock_at`).
        assert_eq!(
            into_tick_slot_ms(tick + 20_000.0, tick, FT8_PERIOD_MS),
            11_000.0
        );
        // Read again in the next slot, the other station's: past the period, never a key there.
        let crossed = into_tick_slot_ms(slot_start + 15_200.0, tick, FT8_PERIOD_MS);
        assert_eq!(crossed, 15_200.0);
        assert!(!wsjtx_may_key_at(crossed, FT8_PERIOD_MS));
    }

    #[test]
    fn a_boundary_decides_on_the_clock_at_its_key_not_on_its_ticks() {
        // On the real (steered) clock: a boundary tick that read its clock 2 s before it reached the
        // key (CAT, a decode's wait). The slot is 11.5 s in by then, so it keys nothing, though its
        // tick read 9.5 s; at 10.5 s (the tick 8.5 s) it keys.
        for (at_ms, keys) in [(11_500.0, false), (10_500.0, true)] {
            let mut eng = armed_engine();
            let offset = eng.clock_offset_ms().unwrap_or(0) - at_ms as i64;
            eng.set_clock_offset_ms(Some(offset));
            assert_eq!(eng.clock_offset_ms(), Some(offset), "premise: steered");
            let (addr, log, _) = keying_rigctld("RPRT 0", 0);
            let mut rig = Rig::rigctld(&addr);
            let mut backend = MockBackend::new();
            let mut rx = RxRing::new();
            let tick_read = steered_now_ms(&eng) - 2_000.0;
            let act = slot_tx_phase(
                &mut eng,
                &mut rig,
                &mut backend,
                &mut rx,
                0,
                tick_read,
                false,
                None,
                Some(vec![stand_in_over()]),
                false,
            );
            let sent = log.lock().unwrap().clone();
            if keys {
                assert_eq!(sent, ["T 1"], "control, {at_ms} ms in: the key");
                assert!(act.tx_this_slot, "control");
            } else {
                assert!(
                    sent.is_empty() && !act.tx_this_slot,
                    "{at_ms} ms in, its tick 2 s behind: keyed {sent:?}"
                );
            }
        }
    }

    #[test]
    fn a_boundary_whose_key_comes_past_75_percent_keys_nothing_and_its_over_waits() {
        // ⭐ A boundary whose key comes past 75 % of the slot (a decode that drained that late, a
        // busy computer) is not keyed, as WSJT-X never asserts PTT there. The over is not planned
        // either, so the same over is still the next to go, for the next own slot (the CQ is a
        // chat broadcast of several frames here: a plan would have spent its first). It used to key
        // as long as one of its tones was still ahead (13.14 s of FT8) and send those. The control,
        // under 75 %, keys and sends what remains of the over on the slot clock, ending on the window.
        let next_text = |eng: &Engine| match eng.next_over_hint(0) {
            Some(tempo_app::engine::TxWaveform::Native { text, .. }) => Some(text),
            _ => None,
        };
        for (tier, late_ms, in_time_ms, window_ms) in [
            (tempo_app::dto::Tier::Ft8, 11_300.0, 11_200.0, 13_640.0),
            (tempo_app::dto::Tier::Ft4, 5_700.0, 5_500.0, 6_040.0),
        ] {
            let what = format!("{tier:?}");
            let mut eng = armed_engine_on(tier);
            let queued = next_text(&eng);
            assert!(queued.is_some(), "{what}: premise: an over to send");
            let (act, backend, sent) = tx_phase_at(&mut eng, late_ms, None);
            assert!(
                sent.is_empty(),
                "{what} {late_ms} ms in: a command went on the wire: {sent:?}"
            );
            assert!(
                !act.tx_this_slot && act.tx_until_ms.is_none() && backend.played.is_empty(),
                "{what} {late_ms} ms in: keyed"
            );
            assert!(eng.tx_enabled(), "{what}: a missed over is not a halt");
            assert_eq!(
                next_text(&eng),
                queued,
                "{what}: the over was planned and spent though it never keyed"
            );

            let whole = armed_engine_on(tier).poll_tx(0).remove(0);
            let mut eng = armed_engine_on(tier);
            let slot_start = steered_slot_start(&eng);
            let (act, backend, sent) = tx_phase_at(&mut eng, in_time_ms, None);
            assert_eq!(sent, ["T 1"], "{what} {in_time_ms} ms in: control: the key");
            let skip = (in_time_ms * 12.0) as usize;
            assert!(
                backend.played == whole[skip..],
                "{what} {in_time_ms} ms in: control: not the rest of the over from there"
            );
            assert_eq!(
                (act.audio_until_ms, act.tx_until_ms),
                (
                    Some(slot_start + window_ms),
                    Some(slot_start + window_ms + 200.0)
                ),
                "{what}: control: not ended on WSJT-X's window"
            );
        }
    }

    #[test]
    fn an_over_built_before_a_wait_that_ends_past_75_percent_is_dropped_with_its_dial_shift() {
        // The busy decoder's over: planned and built at the boundary, then held until the decode let
        // go of the modem. A wait that ends past 75 % of the slot keys nothing (WSJT-X never asserts
        // PTT there), and the dial shift planned for the over goes with it rather than moving the
        // radio for some later over. Split Operation's Fake It: a 750 Hz TX offset shifts the dial.
        for (at_ms, keys) in [(11_300.0, false), (11_200.0, true)] {
            let mut eng = Engine::new("W9XYZ", "EN37", 0);
            eng.set_tier(tempo_app::dto::Tier::Ft8);
            let _ = eng.take_slot_tx_abort();
            let mut st = eng.settings().clone();
            st.split_mode = tempo_app::settings::SplitMode::FakeIt;
            eng.apply_settings(st);
            eng.set_tx_enabled(true);
            eng.set_tx_offset(750.0);
            eng.broadcast("CQ TEST W9XYZ EN37");
            align_to_slot_start(&mut eng, 0);
            let plan = eng.plan_tx(0).expect("premise: the CQ is planned");
            let wave = plan.waveform.build();
            let waves = eng.commit_tx(&plan, wave, 0);
            let (act, _, sent) = tx_phase_at(&mut eng, at_ms, Some(waves));
            if keys {
                assert!(
                    sent.iter().any(|l| l.starts_with("F ")) && sent.iter().any(|l| l == "T 1"),
                    "control {at_ms} ms in: the shifted dial, then the key: {sent:?}"
                );
                assert!(act.tx_this_slot, "control");
            } else {
                assert!(
                    sent.is_empty(),
                    "{at_ms} ms in: a command went on the wire: {sent:?}"
                );
                assert!(!act.tx_this_slot && act.tx_until_ms.is_none());
                assert_eq!(
                    eng.take_tx_dial_shift(),
                    0,
                    "the dropped over's dial shift was left for the next over"
                );
            }
        }
    }

    #[test]
    fn ft2_keeps_its_own_late_key_past_75_percent() {
        // Control: WSJT-X's latest key is FT8's and FT4's. FT2 keeps its own: a boundary whose key
        // comes 80 % into the slot still keys, and plays its whole over from the key. (JS8 keeps
        // JS8Call's late start instead: `js8_slot`'s tests.)
        let tier = tempo_app::dto::Tier::Ft2;
        {
            let mut eng = armed_engine_on(tier);
            let period_ms = eng.active_slot_secs() * 1000.0;
            let (act, backend, sent) =
                tx_phase_at(&mut eng, 0.8 * period_ms, Some(vec![stand_in_over()]));
            assert_eq!(sent, ["T 1"], "{tier:?}: not keyed");
            assert!(
                act.tx_this_slot && backend.played.len() == FT8_WAVE,
                "{tier:?}: not the whole over"
            );
        }
    }

    // ── A key in time with no tone left: held to the window, FT8's and FT4's, as WSJT-X's is ──────
    // FT4's tones end 5.54 s into its slot (Nexus's 0.5 s lead-in), before its latest key at 5.625 s,
    // and any key in time whose carrier comes up after the last tone has none left either. WSJT-X keys
    // all the same (`widgets/mainwindow.cpp:5126-5176`), its Modulator finds every symbol behind it and
    // sends none (`Modulator/Modulator.cpp:96-99`, `:281`), and PTT falls only at the window's end
    // (`widgets/mainwindow.cpp:5180`, `:5530`, `stopTx` `:5743-5753`, `stopTx2` `:5758-5760`).

    #[test]
    fn every_key_in_time_ends_on_the_window_and_only_ft4_can_find_no_tone_left() {
        // ⭐ AT EVERY MILLISECOND WSJT-X'S LATEST KEY ALLOWS, with the real waves: FT8's tones run to
        // 13.14 s, past its 11.25 s, so a key in time always has tones left to play; FT4's end at
        // 5.54 s, before its 5.625 s, so a key between the two has none (the sweep's positive control:
        // it finds the case where it is). There nothing plays and PTT is still held to the window's
        // end, as WSJT-X holds it; it used to drop on the next tick. Every over ends on the window.
        for (tier, window_ms, no_tone_left) in [
            (tempo_app::dto::Tier::Ft8, 13_640.0, None),
            (tempo_app::dto::Tier::Ft4, 6_040.0, Some((5_540, 5_624))),
        ] {
            let eng = armed_engine_on(tier);
            let waves = vec![armed_engine_on(tier).poll_tx(0).remove(0)];
            let period_ms = eng.active_slot_secs() * 1000.0;
            let slot_start = 1_000.0 * period_ms;
            let mut backend = MockBackend::new();
            let mut found = Vec::new();
            for ms in (0..period_ms as i64).filter(|&ms| wsjtx_may_key_at(ms as f64, period_ms)) {
                let keyed = slot_start + ms as f64;
                backend.played.clear();
                let over = play_keyed_over(&eng, &mut backend, &waves, keyed, keyed);
                assert_eq!(
                    (over.audio_until_ms, over.tx_until_ms),
                    (Some(slot_start + window_ms), slot_start + window_ms + 200.0),
                    "{tier:?} keyed {ms} ms in: not held to WSJT-X's window"
                );
                if !backend.played.iter().any(|&x| x != 0.0) {
                    assert!(
                        backend.played.is_empty(),
                        "{tier:?} keyed {ms} ms in: {} samples of silence played",
                        backend.played.len()
                    );
                    found.push(ms);
                }
            }
            let span = found.first().zip(found.last()).map(|(a, b)| (*a, *b));
            assert_eq!(
                span, no_tone_left,
                "{tier:?}: the ms into the slot where a key in time finds no tone left"
            );
            assert_eq!(
                found.len() as i64,
                span.map_or(0, |(a, b)| b - a + 1),
                "{tier:?}: not one run"
            );
        }
    }

    #[test]
    fn an_ft8_over_whose_carrier_came_after_its_last_tone_is_held_to_the_window() {
        // FT8's tones run past its latest key, so it finds none left only when more than 1.89 s pass
        // between a decision in time and its carrier: split's CAT before the key on a slow link, or a
        // late start's build waiting for a decode to let go of the modem. Decided 11.2 s in, its
        // carrier up 2.0 s later at 13.2 s: nothing plays, and PTT is held to 13.84 s (the audio stop
        // at 13.64 s), as WSJT-X holds an over whose PTT was answered late (its Modulator starts on the
        // answer, `widgets/mainwindow.cpp:8845-8856`, and finds nothing left). It used to drop PTT on
        // the next tick.
        let eng = armed_engine_on(tempo_app::dto::Tier::Ft8);
        let waves = vec![armed_engine_on(tempo_app::dto::Tier::Ft8)
            .poll_tx(0)
            .remove(0)];
        let slot_start = 1_000.0 * FT8_PERIOD_MS;
        let (decided, keyed) = (slot_start + 11_200.0, slot_start + 13_200.0);
        assert!(
            wsjtx_may_key_at(decided - slot_start, FT8_PERIOD_MS),
            "premise: decided in time"
        );
        let mut backend = MockBackend::new();
        let over = play_keyed_over(&eng, &mut backend, &waves, keyed, decided);
        assert!(backend.played.is_empty(), "FT8 carrier 13.2 s in: played");
        assert_eq!(
            (over.audio_until_ms, over.tx_until_ms),
            (Some(slot_start + 13_640.0), slot_start + 13_840.0),
            "FT8 carrier 13.2 s in: not held to WSJT-X's window"
        );
    }

    /// A sound card whose transmit open, the last step before the key, takes `open`: a stand-in for
    /// anything that puts the carrier late after a decision in time.
    struct SlowOpen {
        open: std::time::Duration,
        played: Vec<f32>,
    }

    impl AudioBackend for SlowOpen {
        fn capture(&mut self) -> Vec<f32> {
            Vec::new()
        }
        fn play(&mut self, samples: &[f32]) {
            self.played.extend_from_slice(samples);
        }
        fn prepare_tx(&mut self) -> Result<(), String> {
            std::thread::sleep(self.open);
            Ok(())
        }
    }

    #[test]
    fn an_ft8_boundary_whose_carrier_comes_after_its_last_tone_holds_ptt_to_the_window() {
        // FT8's case through the TX phase, on the real (steered) clock: decided 11.2 s into the slot,
        // under its latest key and with tones still ahead; the carrier up 2.4 s later, past the last
        // tone at 13.14 s (split's CAT on a slow link would do it; here the sound card's open). The
        // key goes out, nothing plays, and PTT is held to 13.84 s with the audio stop at 13.64 s, as
        // WSJT-X holds it. It used to drop PTT on the next tick.
        let wave = armed_engine().poll_tx(0).remove(0);
        let mut eng = armed_engine();
        // The wall clock 10.95 s into the slot; the tick read 11.2 s, ahead of it, so the decision is
        // exactly 11.2 s in.
        let offset = eng.clock_offset_ms().unwrap_or(0) - 10_950;
        eng.set_clock_offset_ms(Some(offset));
        assert_eq!(eng.clock_offset_ms(), Some(offset), "premise: steered");
        let slot_start = steered_slot_start(&eng);
        let (addr, log, _) = keying_rigctld("RPRT 0", 0);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = SlowOpen {
            open: std::time::Duration::from_millis(2_400),
            played: Vec::new(),
        };
        let mut rx = RxRing::new();
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            slot_start + 11_200.0,
            false,
            None,
            Some(vec![wave]),
            false,
        );
        assert_eq!(*log.lock().unwrap(), ["T 1"], "not keyed");
        assert!(act.tx_this_slot, "not keyed");
        assert!(
            backend.played.is_empty(),
            "{} samples played: the carrier came after the last tone",
            backend.played.len()
        );
        assert_eq!(
            (act.audio_until_ms, act.tx_until_ms),
            (Some(slot_start + 13_640.0), Some(slot_start + 13_840.0)),
            "not held to WSJT-X's window"
        );
    }

    #[test]
    fn an_ft4_boundary_whose_key_takes_it_past_its_last_tone_holds_ptt_to_the_window() {
        // The same case reached through a boundary, on the real (steered) clock: decided 5.0 s into
        // the slot with tones still ahead, its key answered 600 ms later, the carrier up at 5.6 s with
        // none left. Nothing plays and PTT is held to 6.24 s, as WSJT-X holds an over whose PTT was
        // answered late; it used to drop on the next tick.
        let wave = armed_engine_on(tempo_app::dto::Tier::Ft4)
            .poll_tx(0)
            .remove(0);
        let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft4);
        let offset = eng.clock_offset_ms().unwrap_or(0) - 5_000;
        eng.set_clock_offset_ms(Some(offset));
        assert_eq!(eng.clock_offset_ms(), Some(offset), "premise: steered");
        let mut rig = Rig::rigctld(&slow_rigctld(600));
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        let tick = steered_now_ms(&eng);
        let slot_start = (tick / FT4_PERIOD_MS).floor() * FT4_PERIOD_MS;
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            tick,
            false,
            None,
            Some(vec![wave]),
            false,
        );
        let keyed_at = steered_now_ms(&eng) - slot_start;
        assert!(act.tx_this_slot, "premise: keyed");
        assert!(keyed_at > 5_540.0, "premise: the key came at {keyed_at} ms");
        assert!(
            backend.played.is_empty(),
            "{} samples played",
            backend.played.len()
        );
        assert_eq!(
            (act.audio_until_ms, act.tx_until_ms),
            (Some(slot_start + 6_040.0), Some(slot_start + 6_240.0)),
            "not held to WSJT-X's window"
        );
    }

    #[test]
    fn an_over_with_no_tone_left_whose_build_ran_past_its_latest_key_is_keyed_and_held() {
        // ⭐ A BOUNDARY DECIDED IN TIME IS KEYED HOWEVER LONG ITS BUILD THEN TAKES, AND HELD SILENT TO
        // THE WINDOW'S END, as WSJT-X's GUI tick keys at its decision (`widgets/mainwindow.cpp:5126-
        // 5176`) and holds it. An FT4 boundary decided 5.60 s into its slot (under 5.625 s, its tones
        // already gone) whose build waits for a decode to let go of the modem until 5.70 s, past its
        // latest key: the key, nothing played, the audio stop at 6.04 s and PTT off at 6.24 s. It
        // used to key nothing.
        //
        // An FT8 or FT4 over is built without the modem (`modes::wsjt77`) unless its text is one the
        // lock-free encoder leaves to the vendored one, which only typed text reaches: here in-QSO
        // free text naming an unresolved hashed call.
        //
        // ⚠️ THE MODEM LOCK IS THE WHOLE PROCESS'S: held here for about 250 ms, the decision's build
        // only.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::Ft4);
        let _ = eng.take_slot_tx_abort();
        eng.set_tx_enabled(true);
        eng.set_mode("qso-run").expect("premise: a CQ run");
        eng.qso_freetext("<...> W9XYZ -10");
        match eng.next_over_hint(0) {
            Some(tempo_app::engine::TxWaveform::Native { text, .. }) => assert!(
                tempo_core::modes::wsjt77::ft4_tones(&text).is_none(),
                "premise: the lock-free encoder builds {text:?}, so nothing waits on the modem"
            ),
            _ => panic!("premise: the free text is not the next over"),
        }
        let (holding, held) = std::sync::mpsc::channel();
        let (until_tx, until_rx) = std::sync::mpsc::channel::<(f64, f64)>();
        let decode = std::thread::spawn(move || {
            let modem = loop {
                match tempo_core::modes::Ft8A7ResetGuard::try_acquire() {
                    Some(modem) => break modem,
                    None => std::thread::sleep(std::time::Duration::from_millis(1)),
                }
            };
            holding.send(()).unwrap();
            let (offset, until) = until_rx.recv().unwrap();
            let give_up = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while now_unix_ms() - offset < until && std::time::Instant::now() < give_up {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            drop(modem);
            std::time::Instant::now()
        });
        held.recv().unwrap();
        // The wall clock 5.45 s into the slot; the tick read 5.60 s, ahead of it, so the decision is
        // exactly 5.60 s in.
        align_to_slot_start(&mut eng, 0);
        let offset = eng.clock_offset_ms().unwrap_or(0) - 5_450;
        eng.set_clock_offset_ms(Some(offset));
        assert_eq!(eng.clock_offset_ms(), Some(offset), "premise: steered");
        let slot_start = steered_slot_start(&eng);
        until_tx
            .send((offset as f64, slot_start + 5_700.0))
            .unwrap();
        let (act, backend, sent) = tx_phase_at(&mut eng, 5_600.0, None);
        let returned = std::time::Instant::now();
        let let_go = decode.join().unwrap();
        assert!(
            returned >= let_go,
            "premise: the over was planned and its build waited for the decode"
        );
        assert_eq!(sent, ["T 1"], "not keyed");
        assert!(
            act.tx_this_slot && backend.played.is_empty(),
            "{} samples played: FT4's tones end at 5.54 s",
            backend.played.len()
        );
        assert_eq!(
            (act.audio_until_ms, act.tx_until_ms),
            (Some(slot_start + 6_040.0), Some(slot_start + 6_240.0)),
            "not held to WSJT-X's window"
        );
        assert!(eng.tx_enabled(), "TX went off");
    }

    #[test]
    fn an_over_with_no_tone_left_whose_clock_has_left_its_ticks_slot_keys_nothing() {
        // A tick that read its clock 5.58 s into an FT4 slot two periods before the decision (a clock
        // step, or a stall the slot code cannot tell from one). The decision falls back to the tick's
        // clock (`into_tick_slot_ms`) and finds it under the latest key; the over has no tone left
        // there. Its latest key is looked at on the clock as it is now, from the start of the tick's
        // slot, so it is long past: it keys nothing, as before, rather than a key whose window ended
        // before it, in a slot that is not its own.
        //
        // LEFT AS IT WAS, and not settled here: an over with a tone left in the same place (5.0 s)
        // still keys and plays from where its tick put it.
        for (into_ms, keys) in [(5_580.0, false), (5_000.0, true)] {
            let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft4);
            let tick = steered_slot_start(&eng) - 2.0 * FT4_PERIOD_MS + into_ms;
            let (addr, log, _) = keying_rigctld("RPRT 0", 0);
            let mut rig = Rig::rigctld(&addr);
            let mut backend = MockBackend::new();
            let mut rx = RxRing::new();
            let act = slot_tx_phase(
                &mut eng,
                &mut rig,
                &mut backend,
                &mut rx,
                0,
                tick,
                false,
                None,
                Some(vec![ft4_stand_in()]),
                false,
            );
            let sent = log.lock().unwrap().clone();
            if keys {
                assert_eq!(sent, ["T 1"], "a tone left, {into_ms} ms in: not keyed");
                assert!(
                    backend.played == ft4_stand_in()[(into_ms * 12.0) as usize..],
                    "a tone left, {into_ms} ms in: not the rest of the over from the tick's place"
                );
            } else {
                assert!(
                    sent.is_empty() && !act.tx_this_slot && act.tx_until_ms.is_none(),
                    "no tone left, {into_ms} ms in: keyed {sent:?}"
                );
                assert!(backend.played.is_empty(), "no tone left: played");
            }
        }
    }

    // ── An FT8/FT4 key the radio answers past the end of its slot: nothing sent, unkeyed at once ──────
    // Split Operation's CAT and the key itself come after the last slot check before an FT8/FT4 key,
    // and on a slow link they take seconds: three split commands, each up to its 2.5 s deadline, and
    // the key's own 700 ms. The tones are laid on the clock where the carrier is, so a carrier that
    // came up in the other station's period sent the rest of the over there, on their slot clock.
    // WSJT-X starts an over's audio only when the radio reports PTT on and the over is still needed
    // (`widgets/mainwindow.cpp:8852`), and its GUI tick ends the over as soon as the clock has left its
    // window (`:5055`, `:5180`, `:5530`), a window in this station's own period (`:5016-5019`).

    type LinesWithTimes = std::sync::Arc<std::sync::Mutex<Vec<(String, std::time::Instant)>>>;

    /// A rigctld on a slow link: each of Split Operation's commands (`S`, `I`, `X`) answered after
    /// `split_ms`, the key after `key_ms`, everything else at once; every line logged with when it
    /// came. Each connection is served on a thread of its own, so a command that outlives its
    /// deadline (the client then drops the connection) holds up nothing after it.
    fn slow_split_rigctld(split_ms: u64, key_ms: u64) -> (String, LinesWithTimes) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_w = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = log_w.clone();
                std::thread::spawn(move || {
                    let mut w = stream.try_clone().unwrap();
                    for line in BufReader::new(stream).lines().map_while(Result::ok) {
                        let t = line.trim().to_string();
                        if t.is_empty() {
                            continue;
                        }
                        log.lock()
                            .unwrap()
                            .push((t.clone(), std::time::Instant::now()));
                        let stall = if ["S ", "I ", "X "].iter().any(|p| t.starts_with(p)) {
                            split_ms
                        } else if t == "T 1" {
                            key_ms
                        } else {
                            0
                        };
                        std::thread::sleep(std::time::Duration::from_millis(stall));
                        if w.write_all(b"RPRT 0\n").is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (addr, log)
    }

    /// An engine on `tier` in Split Operation Rig with a CQ to send whose 750 Hz TX offset needs a
    /// dial shift, so three split commands go out before its key; its clock steered to a slot start.
    fn rig_split_engine(tier: tempo_app::dto::Tier) -> Engine {
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tier);
        let _ = eng.take_slot_tx_abort();
        let mut st = eng.settings().clone();
        st.split_mode = tempo_app::settings::SplitMode::Rig;
        eng.apply_settings(st);
        eng.set_tx_enabled(true);
        eng.set_tx_offset(750.0);
        eng.broadcast("CQ TEST W9XYZ EN37");
        align_to_slot_start(&mut eng, 0);
        eng
    }

    /// [`rig_split_engine`] running Call CQ: a CQ run, planned again at every own boundary while TX
    /// is on, as WSJT-X sends its Tx6 every period while Enable Tx is ticked. Call CQ's snappy
    /// first over is drained: the boundary keys it.
    fn rig_split_cq_run(tier: tempo_app::dto::Tier) -> Engine {
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tier);
        let _ = eng.take_slot_tx_abort();
        let mut st = eng.settings().clone();
        st.split_mode = tempo_app::settings::SplitMode::Rig;
        eng.apply_settings(st);
        eng.set_tx_offset(750.0);
        eng.start_cq(None).expect("premise: Call CQ");
        let _ = eng.take_immediate_tx();
        align_to_slot_start(&mut eng, 0);
        eng
    }

    /// The dropped-over lane in `eng`'s snapshot, as the UI reads it, or `None` while it is absent.
    fn over_dropped(eng: &Engine) -> Option<serde_json::Value> {
        serde_json::to_value(eng.snapshot()).unwrap()["radio"]
            .get("slotOverDropped")
            .cloned()
    }

    /// A boundary of `tier` decided `decided_ms` into its slot, on the real (steered) clock, on a link
    /// whose split commands answer after `split_ms` each and whose key after `key_ms`.
    struct SlowSplit {
        act: SlotAction,
        eng: Engine,
        played: Vec<f32>,
        /// The over the scene sends, whole and slot-positioned.
        wave: Vec<f32>,
        /// Every line the rigctld got, with where the slot was when it came, ms from its start.
        lines: Vec<(String, f64)>,
        /// Where the slot was when the TX phase returned.
        returned_ms: f64,
        slot_start: f64,
        period_ms: f64,
    }

    fn slow_split_boundary(
        tier: tempo_app::dto::Tier,
        decided_ms: f64,
        split_ms: u64,
        key_ms: u64,
    ) -> SlowSplit {
        let wave = rig_split_engine(tier).poll_tx(0).remove(0);
        slow_split_boundary_of(
            rig_split_engine(tier),
            wave,
            0,
            decided_ms,
            (split_ms, key_ms),
            false,
        )
    }

    /// [`slow_split_boundary`] for `eng` at its boundary `slot`, sending `wave`, on a link whose
    /// split commands answer after `split_ms` each and whose key after `key_ms`. `held`: the loop
    /// already holds the transmitter (the operator's own PTT).
    fn slow_split_boundary_of(
        mut eng: Engine,
        wave: Vec<f32>,
        slot: u64,
        decided_ms: f64,
        (split_ms, key_ms): (u64, u64),
        held: bool,
    ) -> SlowSplit {
        let period_ms = eng.active_slot_secs() * 1000.0;
        let (addr, log) = slow_split_rigctld(split_ms, key_ms);
        let mut rig = Rig::rigctld(&addr);
        rig.set_slow_transport(true);
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        // Steered last, so a scene decided near WSJT-X's latest key is decided there to within the
        // microseconds before `slot_tx_phase` reads its clock, not after the set-up above.
        align_to_slot_start(&mut eng, 0);
        let offset = eng.clock_offset_ms().unwrap_or(0) - decided_ms as i64;
        eng.set_clock_offset_ms(Some(offset));
        assert_eq!(eng.clock_offset_ms(), Some(offset), "premise: steered");
        let (base, tick) = (std::time::Instant::now(), steered_now_ms(&eng));
        let slot_start = (tick / period_ms).floor() * period_ms;
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            slot,
            tick,
            false,
            None,
            None,
            held,
        );
        let at = |t: std::time::Instant| tick - slot_start + (t - base).as_secs_f64() * 1000.0;
        let returned_ms = at(std::time::Instant::now());
        let lines = log
            .lock()
            .unwrap()
            .iter()
            .map(|(l, t)| (l.clone(), at(*t)))
            .collect();
        SlowSplit {
            act,
            eng,
            played: backend.played,
            wave,
            lines,
            returned_ms,
            slot_start,
            period_ms,
        }
    }

    impl SlowSplit {
        /// The lines the rigctld got, without their times.
        fn sent(&self) -> Vec<&str> {
            self.lines.iter().map(|(l, _)| l.as_str()).collect()
        }

        /// Where in the slot `line` came, the first time.
        fn came(&self, line: &str) -> f64 {
            self.lines
                .iter()
                .find(|(l, _)| l == line)
                .unwrap_or_else(|| panic!("no {line:?} on the wire: {:?}", self.sent()))
                .1
        }

        /// The over went out in its own period: the key, the rest of its wave from where the
        /// carrier came up (between the key's answer and the TX phase's return) and not a sample
        /// after, the over ended on `window_ms` (`None`: it had nothing left, and ended at once).
        fn played_in_its_own_period(&mut self, key_ms: u64, window_ms: Option<f64>, what: &str) {
            assert_eq!(
                self.sent()[3..],
                ["T 1"],
                "{what}: the split, the key, and no unkey"
            );
            let answered = self.came("T 1") + key_ms as f64;
            assert!(
                self.returned_ms < self.period_ms,
                "premise, {what}: the carrier came up {answered}..{} ms into its slot",
                self.returned_ms
            );
            let skipped = self.wave.len() - self.played.len();
            assert!(
                self.played == self.wave[skipped..],
                "{what}: not the rest of the over"
            );
            match window_ms {
                Some(window_ms) => {
                    let (lo, hi) = (
                        (answered * 12.0).floor() as usize,
                        (self.returned_ms * 12.0).ceil() as usize,
                    );
                    assert!(
                        self.played.iter().any(|&x| x != 0.0) && (lo..=hi).contains(&skipped),
                        "{what}: {skipped} samples skipped, the carrier came up {lo}..={hi} samples in"
                    );
                    assert_eq!(
                        (self.act.audio_until_ms, self.act.tx_until_ms),
                        (
                            Some(self.slot_start + window_ms),
                            Some(self.slot_start + window_ms + 200.0)
                        ),
                        "{what}: not ended on WSJT-X's window"
                    );
                }
                None => {
                    let end = self.act.tx_until_ms.expect("keyed") - self.slot_start;
                    assert!(
                        self.played.is_empty()
                            && self.act.audio_until_ms == self.act.tx_until_ms
                            && (answered..=self.returned_ms).contains(&end),
                        "{what}: {} samples played, the over ending {end} ms in",
                        self.played.len()
                    );
                }
            }
            assert!(
                self.act.tx_this_slot && self.eng.tx_enabled(),
                "{what}: dropped"
            );
            assert_eq!(self.eng.snapshot().radio.slot_key_refused, None, "{what}");
        }

        /// The carrier came up past the end of its slot: nothing played, the unkey at once after
        /// the key's answer (none while `held`: the operator's own PTT stays theirs), and the split
        /// handed back for the loop to tear down. Returns when the key was answered, ms into the
        /// slot.
        fn sent_nothing_and_unkeyed_at_once(&self, key_ms: u64, held: bool, what: &str) -> f64 {
            let answered = self.came("T 1") + key_ms as f64;
            assert!(
                answered >= self.period_ms,
                "premise, {what}: the carrier came up {answered} ms into its slot"
            );
            assert!(
                self.played.is_empty(),
                "{what}: {} samples played in a later period, the over ending {:?} ms into it",
                self.played.len(),
                self.act
                    .tx_until_ms
                    .map(|t| t - self.slot_start - self.period_ms)
            );
            if held {
                assert_eq!(
                    self.sent()[3..],
                    ["T 1"],
                    "{what}: the split and the key, and the operator's PTT left to them"
                );
            } else {
                assert_eq!(
                    self.sent()[3..],
                    ["T 1", "T 0"],
                    "{what}: the split, the key, then the unkey"
                );
                assert!(
                    self.came("T 0") - answered < 250.0,
                    "{what}: the unkey came {} ms after the key's answer",
                    self.came("T 0") - answered
                );
            }
            assert!(
                !self.act.tx_this_slot && self.act.tx_until_ms.is_none(),
                "{what}: held"
            );
            assert!(self.act.rig_split_engaged, "{what}: the split was lost");
            answered
        }

        /// The dropped-over lane says how late the radio keyed: within the key's answer and the TX
        /// phase's return, past the end of the period.
        fn says_how_late(&self, lane: &serde_json::Value, answered: f64, what: &str) {
            let late_s: f64 = lane["late"]
                .as_str()
                .and_then(|late| late.trim_end_matches(" s").parse().ok())
                .unwrap_or_else(|| panic!("{what}: how late? {lane}"));
            let (lo, hi) = (
                (answered - self.period_ms) / 1000.0 - 0.05,
                (self.returned_ms - self.period_ms) / 1000.0 + 0.05,
            );
            assert!(
                (lo..=hi).contains(&late_s),
                "{what}: {lane}, expected {lo:.2}..={hi:.2} s past its period"
            );
        }

        /// ⭐ ONLY THIS OVER IS DROPPED, AND TX STAYS ON, as WSJT-X starts no audio for it and
        /// leaves Enable Tx ticked for the next: no stop for the loop, nothing on the refused key's
        /// line, and a lane of its own saying the over was dropped, how late the radio keyed it,
        /// and how many in a row (`in_a_row`).
        fn dropped_with_tx_left_on(&mut self, answered: f64, in_a_row: u64, what: &str) {
            assert!(self.eng.tx_enabled(), "{what}: TX went off");
            assert!(
                !self.eng.take_slot_tx_abort(),
                "{what}: the loop was told to stop"
            );
            assert_eq!(
                self.eng.snapshot().radio.slot_key_refused,
                None,
                "{what}: on the refused key's line"
            );
            let lane = over_dropped(&self.eng)
                .unwrap_or_else(|| panic!("{what}: nothing says the over was dropped"));
            self.says_how_late(&lane, answered, what);
            assert_eq!(lane["count"], in_a_row, "{what}: {lane}");
        }

        /// [`Self::sent_nothing_and_unkeyed_at_once`] and [`Self::dropped_with_tx_left_on`].
        fn dropped_at_once(&mut self, key_ms: u64, in_a_row: u64, what: &str) {
            let answered = self.sent_nothing_and_unkeyed_at_once(key_ms, false, what);
            self.dropped_with_tx_left_on(answered, in_a_row, what);
        }

        /// Keyed in its period with no tone left: nothing played, PTT held silent to WSJT-X's
        /// window (`window_ms`, then PTT 200 ms later), TX on.
        fn held_silent_to_the_window(&self, window_ms: f64, what: &str) {
            assert_eq!(
                self.sent()[3..],
                ["T 1"],
                "{what}: the split, the key, and no unkey"
            );
            assert!(
                self.returned_ms < self.period_ms,
                "premise, {what}: the carrier came up {} ms into its slot",
                self.returned_ms
            );
            assert!(
                self.act.tx_this_slot && self.played.is_empty(),
                "{what}: {} samples played",
                self.played.len()
            );
            assert_eq!(
                (self.act.audio_until_ms, self.act.tx_until_ms),
                (
                    Some(self.slot_start + window_ms),
                    Some(self.slot_start + window_ms + 200.0)
                ),
                "{what}: not held to WSJT-X's window"
            );
            assert!(self.eng.tx_enabled(), "{what}: TX went off");
        }
    }

    /// The split's three commands and the key answered late enough that the carrier comes 2 s, 6 s
    /// and 7.8 s after the decision: each command after `split_ms`, the key after `key_ms`. Every
    /// one inside its CAT deadline (2.5 s, 0.7 s): a split write left unanswered keys nothing at all
    /// (`a_split_write_the_radio_does_not_answer_sends_nothing_and_halts_tx`).
    const SLOW_SPLITS: [(u64, u64); 3] = [(600, 200), (1_900, 300), (2_400, 600)];

    #[test]
    fn an_ft8_key_a_slow_split_takes_past_its_period_sends_nothing_and_is_unkeyed_at_once() {
        // ⭐ A boundary decided 10.0 s into this station's FT8 slot (under 11.25 s), its split and
        // key taking 2 s, 6 s and 7.8 s. 2 s: the carrier at 12.0 s, the rest of the over from
        // there on the slot clock, ended at 13.64 / 13.84 s. 6 s and 7.8 s: the carrier at 16.0 s and
        // 17.8 s, in the other station's period, where the over used to go out from their slot's 1.0
        // s and 2.8 s, its tones on their clock and PTT held to their 13.84 s; now nothing is played
        // and the radio is unkeyed at once. Only that over is dropped: TX stays on, and its own
        // lane says so.
        let mut scenes = std::thread::scope(|s| {
            SLOW_SPLITS
                .map(|(split, key)| {
                    s.spawn(move || {
                        slow_split_boundary(tempo_app::dto::Tier::Ft8, 10_000.0, split, key)
                    })
                })
                .map(|h| h.join().unwrap())
        });
        scenes[0].played_in_its_own_period(200, Some(13_640.0), "FT8, 2 s");
        scenes[1].dropped_at_once(300, 1, "FT8, 6 s");
        scenes[2].dropped_at_once(600, 1, "FT8, 7.8 s");
    }

    #[test]
    fn an_ft4_key_a_slow_split_takes_past_its_period_sends_nothing_and_is_unkeyed_at_once() {
        // ⭐ A boundary decided 0.5 s into this station's FT4 slot, its split and key taking 2 s, 6
        // s and 7.8 s. 2 s: the carrier at 2.5 s, the rest of the over on the slot clock, ended at
        // 6.04 / 6.24 s. 6 s: the carrier at 6.5 s, past its window but in its own period: nothing
        // left to send, and PTT drops at once, as before. 7.8 s: the carrier at 8.3 s, in the other
        // station's period, and more than a period after the tick, where the post-key clock read is
        // laid on the tick's clock (`slot_clock_at`): the whole over used to go out from the tick's
        // 0.5 s, in the other station's period. Now nothing is played, the radio is unkeyed at
        // once, and only that over is dropped: TX stays on.
        let mut scenes = std::thread::scope(|s| {
            SLOW_SPLITS
                .map(|(split, key)| {
                    s.spawn(move || {
                        slow_split_boundary(tempo_app::dto::Tier::Ft4, 500.0, split, key)
                    })
                })
                .map(|h| h.join().unwrap())
        });
        scenes[0].played_in_its_own_period(200, Some(6_040.0), "FT4, 2 s");
        scenes[1].played_in_its_own_period(300, None, "FT4, 6 s");
        scenes[2].dropped_at_once(600, 1, "FT4, 7.8 s");
    }

    #[test]
    fn a_fast_key_on_split_still_plays_its_over_on_time() {
        // Control: the same split answered at once. The FT8 over keyed on the boundary plays whole
        // from the slot's start and ends on WSJT-X's window, as before.
        let mut fast = slow_split_boundary(tempo_app::dto::Tier::Ft8, 0.0, 0, 0);
        fast.played_in_its_own_period(0, Some(13_640.0), "FT8, a fast key");
        assert!(
            fast.played.len() + 12 * 100 > fast.wave.len(),
            "{} of {} samples played: not the whole over",
            fast.played.len(),
            fast.wave.len()
        );
    }

    // ── Only the over that keyed too late is dropped, and TX stays on ────────────────────────────
    // WSJT-X starts an over's audio only if it is still needed when the radio reports PTT on
    // (`widgets/mainwindow.cpp:8852`, `m_tx_when_ready && g_iptt`). Its `stopTx` ends that over and
    // nothing else (`:5743-5756`: `g_iptt=0`, PTT off 200 ms later); Enable Tx stays ticked, so its
    // next own period asserts PTT again (`:5126`, `:5176`). Only Stop TX unticks it (`:8714-8717`).

    #[test]
    fn a_dropped_over_leaves_tx_on_and_the_next_own_slot_sends_a_full_over() {
        // ⭐ A Call CQ run in Split Operation Rig. Its FT8 boundary decided 10.0 s in, the split and
        // the key taking 6 s (the carrier at 16.0 s), and its FT4 one decided 0.5 s in, taking 7.8 s
        // (8.3 s): nothing is sent and the radio is unkeyed at once, as before, and TX stays on. At
        // the next own boundary, the link answering at once again, the CQ goes out whole and on
        // time, ended on WSJT-X's window, and the lane goes. TX used to turn off at the first.
        fn scene(tier: tempo_app::dto::Tier, decided_ms: f64, link: (u64, u64), window_ms: f64) {
            let what = format!("{tier:?}, {decided_ms} ms in, {link:?}");
            let wave = rig_split_cq_run(tier).poll_tx(0).remove(0);
            let mut late = slow_split_boundary_of(
                rig_split_cq_run(tier),
                wave.clone(),
                0,
                decided_ms,
                link,
                false,
            );
            late.dropped_at_once(link.1, 1, &what);
            let mut next = slow_split_boundary_of(late.eng, wave, 2, 0.0, (0, 0), false);
            next.played_in_its_own_period(
                0,
                Some(window_ms),
                &format!("{what}, the next own slot"),
            );
            assert!(
                next.played.len() + 12 * 100 > next.wave.len(),
                "{what}: {} of {} samples played at the next own slot: not the whole over",
                next.played.len(),
                next.wave.len()
            );
            assert_eq!(
                over_dropped(&next.eng),
                None,
                "{what}: the lane is still up after an over that went out"
            );
        }
        std::thread::scope(|s| {
            s.spawn(|| scene(tempo_app::dto::Tier::Ft8, 10_000.0, (1_900, 300), 13_640.0));
            s.spawn(|| scene(tempo_app::dto::Tier::Ft4, 500.0, (2_400, 600), 6_040.0));
        });
    }

    #[test]
    fn a_key_decided_late_in_the_window_is_dropped_only_when_it_lands_past_its_period() {
        // A boundary decided just under WSJT-X's latest key: FT8 11.2 s in (11.25 s), and FT4 5.6 s
        // in (5.625 s), where FT4 has no tone left (its tones end at 5.54 s) and is keyed and held
        // silent to its window's end. Both overs are built by the lock-free encoder, so nothing
        // between the decision and the key waits on the modem: only the CAT. Controls, the link
        // answering at once: FT8 sends what remains from 11.2 s and ends on its window, and FT4
        // keys and holds PTT silent to 6.24 s, as before. The split and key taking 3.9 s (FT8, the
        // carrier at 15.1 s) or 2.3 s (FT4, 7.9 s) put the carrier in the other station's period:
        // dropped, TX on.
        let text = "CQ TEST W9XYZ EN37";
        assert!(
            tempo_core::modes::wsjt77::ft8_tones(text).is_some()
                && tempo_core::modes::wsjt77::ft4_tones(text).is_some(),
            "premise: {text:?} waits on the modem"
        );
        use tempo_app::dto::Tier::{Ft4, Ft8};
        let [mut ft8, mut ft8_slow, ft4, mut ft4_slow] = std::thread::scope(|s| {
            [
                (Ft8, 11_200.0, (0, 0)),
                (Ft8, 11_200.0, (1_200, 300)),
                (Ft4, 5_600.0, (0, 0)),
                (Ft4, 5_600.0, (700, 200)),
            ]
            .map(|(tier, decided, (split, key))| {
                s.spawn(move || slow_split_boundary(tier, decided, split, key))
            })
            .map(|h| h.join().unwrap())
        });
        ft8.played_in_its_own_period(0, Some(13_640.0), "FT8, 11.2 s, a fast key");
        ft8_slow.dropped_at_once(300, 1, "FT8, 11.2 s, 3.9 s");
        ft4.held_silent_to_the_window(6_040.0, "FT4, 5.6 s, a fast key");
        ft4_slow.dropped_at_once(200, 1, "FT4, 5.6 s, 2.3 s");
    }

    #[test]
    fn a_key_past_its_period_under_the_operators_own_ptt_is_dropped_and_leaves_their_ptt_alone() {
        // The loop already holds the transmitter: the operator's own PTT. A boundary decided 10.0 s
        // into an FT8 slot whose split and key take 6 s is dropped as any other (nothing sent, TX
        // on, the lane), and no unkey goes to the radio: the PTT is the operator's to let go. The
        // halt that turned TX off here used to drop their latched PTT with it.
        let wave = rig_split_engine(tempo_app::dto::Tier::Ft8)
            .poll_tx(0)
            .remove(0);
        let what = "FT8, 6 s, under the operator's PTT";
        let mut held = slow_split_boundary_of(
            rig_split_engine(tempo_app::dto::Tier::Ft8),
            wave,
            0,
            10_000.0,
            (1_900, 300),
            true,
        );
        let answered = held.sent_nothing_and_unkeyed_at_once(300, true, what);
        held.dropped_with_tx_left_on(answered, 1, what);
    }

    #[test]
    fn ft4_overs_dropped_in_a_row_leave_tx_on_and_the_lane_counts_them() {
        // ⭐ HOWEVER MANY OVERS IN A ROW THE RADIO KEYS PAST THEIR PERIOD, TX STAYS ON, AS IN WSJT-X,
        // whose next own period asserts PTT again until its Tx watchdog (6 minutes by default,
        // `widgets/mainwindow.cpp:5109-5111`) ends the run. A Call CQ run on FT4 whose boundaries are
        // each decided 5.6 s in, the split and the key taking 2.3 s (the carrier at 7.9 s): four in a
        // row are each dropped, nothing sent and unkeyed at once, with TX left on and the lane
        // counting them.
        let wave = rig_split_cq_run(tempo_app::dto::Tier::Ft4)
            .poll_tx(0)
            .remove(0);
        let mut eng = rig_split_cq_run(tempo_app::dto::Tier::Ft4);
        for (in_a_row, slot) in [(1, 0), (2, 2), (3, 4), (4, 6)] {
            let what = format!("FT4, drop {in_a_row}");
            let mut dropped =
                slow_split_boundary_of(eng, wave.clone(), slot, 5_600.0, (700, 200), false);
            dropped.dropped_at_once(200, in_a_row, &what);
            eng = dropped.eng;
        }
    }

    #[test]
    fn ptt_hold_is_measured_from_the_key_not_the_ticks_stale_clock() {
        // THE BUG. `now_ms` is bound at the TOP of the radio-loop tick
        // (`service.rs`), and that same tick then runs BLOCKING CAT before the
        // carrier is ever up: the retune block (its `can_retune` gate is true at a
        // boundary, `tx_until_ms` being None) and `ensure_commanded`, each able to
        // spend a full CAT deadline (700 ms, 2500 ms on a slow transport). By the
        // time we key, `now_ms` is that far in the past — so a deadline built on it
        // unkeys EARLY by exactly the CAT time and truncates the tail of the over.
        //
        // MSK144-15 is where it shows: 14.688 s of audio in a 15 s slot leaves
        // ~0.31 s of slack, less than one slow CAT exchange. Every other tier has
        // enough slack to absorb it as a merely shortened tail.
        const CAT_STALL_MS: f64 = 400.0;
        // FT2: the key-relative deadline this guards is every slot tier's but FT8's and FT4's
        // (see `armed_engine_on`).
        let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft2);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        // The tick bound `now` CAT_STALL_MS ago; the blocking CAT that followed is
        // what ate the gap between then and this call.
        let steered = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            steered - CAT_STALL_MS,
            false,
            None,
            None,
            false,
        );

        let after = steered_now_ms(&eng);

        assert!(act.tx_this_slot, "the CQ keyed");
        let basis = deadline_basis_ms(&act, &backend);
        // ⚠️ BRACKETED, NOT A TOLERANCE. This used to assert `(basis - steered).abs() < 100`,
        // which measured how long THIS MACHINE took to run the call: on a loaded box
        // `slot_tx_phase` outruns 100 ms and the test failed for a reason unrelated to the bug
        // it guards (seen at 211 ms and 278 ms). A timing test that fails under load teaches
        // people to re-run it, which is how the regression it exists to catch gets waved
        // through. The claim that actually matters is an ORDERING one and it needs no
        // tolerance at all: the basis must fall inside the window this call spanned. The bug
        // puts it CAT_STALL_MS before `steered`, i.e. before the window opened, so this still
        // fails on the defect while being immune to how busy the machine is.
        assert!(
            basis >= steered && basis <= after,
            "PTT-hold deadline must be measured from the key — expected it inside the window \
             this call spanned ({steered:.0}..{after:.0}), got {basis:.0}. The stale-tick bug \
             puts it at {:.0}, {:.0} ms before the window opened.",
            steered - CAT_STALL_MS,
            steered - basis,
        );
    }

    #[test]
    fn a_slow_key_pushes_the_hold_out_by_the_time_the_key_itself_took() {
        // The same defect measured on the OTHER side of the call: the CAT round-trip
        // of `rig.ptt(true)` is itself inside the window. A fix that re-read the
        // clock on entry (rather than after the key) would still unkey early by the
        // keying round-trip. Here the rig answers `T 1` only after KEY_MS.
        const KEY_MS: u64 = 300;
        // FT2: the key-relative deadline this guards is every slot tier's but FT8's and FT4's
        // (see `armed_engine_on`).
        let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft2);
        let mut rig = Rig::rigctld(&slow_rigctld(KEY_MS));
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let now_ms = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            now_ms,
            false,
            None,
            None,
            false,
        );
        let after = steered_now_ms(&eng);

        assert!(act.tx_this_slot, "the CQ keyed");
        assert!(rig.keyed, "PTT asserted");
        let basis = deadline_basis_ms(&act, &backend);
        assert!(
            basis - now_ms >= 200.0,
            "a {KEY_MS} ms key must push the hold out with it — moved only {:.0} ms",
            basis - now_ms,
        );
        // Bracketed for the same reason as the sibling test above: `(after - basis).abs() <
        // 100` was measuring machine load, not behaviour. The basis must simply lie at or
        // before the moment the call returned, and after the key that pushed it out.
        assert!(
            basis <= after,
            "and the basis is the moment the carrier came up, not something later than this \
             call returned ({:.0} ms after)",
            basis - after,
        );
    }

    #[test]
    fn the_hold_clock_is_utc_steered_like_the_ticks_clock_is() {
        // THE TRAP. `now_ms` is UTC-STEERED: `service.rs` SUBTRACTS the measured
        // PC-clock-vs-UTC offset so TX keys on the true UTC grid even when the OS
        // clock is skewed. Re-reading the raw system clock instead puts the deadline
        // on a DIFFERENT timebase and shifts it by the whole offset — silently, and
        // worst on exactly the badly-synced machines the steering exists to rescue.
        // A skew of tens of seconds is ordinary on a PC that has never talked to
        // an NTP server. 30 s (two whole FT8 periods) rather than the 90 s this
        // test used until 2026-09: guard 3 (`clocksync::MAX_STEER_MS`) refuses to
        // steer by more than 60 s, and `align_to_slot_start` adds up to one
        // period of phase on top. The subject of this test is the TIMEBASE, not
        // the magnitude — 30 s is still 100× TempoFast's whole −0.30 s budget, so
        // a deadline built on the wrong clock still misses by two whole periods.
        // Guard 3's own behaviour is proven in `clocksync`, both directions.
        const OFFSET_MS: i64 = 30_000; // PC clock ~30 s AHEAD of true UTC
        const CAT_STALL_MS: f64 = 400.0;
        // FT2: the key-relative deadline this guards is every slot tier's but FT8's and FT4's
        // (see `armed_engine_on`).
        let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft2);
        // Whole periods, so the skew stays ~30 s while the steered clock still lands
        // on a boundary — see `align_to_slot_start`. 30 s is two whole FT8 periods.
        align_to_slot_start(&mut eng, OFFSET_MS);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let steered = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            steered - CAT_STALL_MS,
            false,
            None,
            None,
            false,
        );

        let after = steered_now_ms(&eng);

        let basis = deadline_basis_ms(&act, &backend);
        // ⚠️ BRACKETED, NOT A TOLERANCE — the same fix the two sibling basis tests got in
        // 8092abfa, which named THIS test in its subject and body but patched
        // `a_slow_key_pushes_the_hold_out_by_the_time_the_key_itself_took` instead, leaving
        // this one on the original `.abs() < 150`. A tolerance around a single pre-call
        // reading measures how long THIS MACHINE took to run the call, not which clock the
        // hold was built on: 3 ms idle here, but 32 ms at a ninth of a core and 256 ms under
        // real contention, where it failed 6 runs in 12 — having already ambushed two agents
        // at 155 ms.
        //
        // The window is the whole claim, and it needs no tolerance: the basis must lie
        // between a reading taken before the call and one taken after it. As a PREDICATE
        // that is strictly tighter — `.abs() < 150` admits a basis up to 150 ms before
        // `steered`, this admits none — but do not claim it closes a hole a real defect
        // could have used, because it was measured and it does not. `align_to_slot_start`
        // puts `steered` on a boundary, so a key landing even slightly before it falls in
        // the PREVIOUS slot and `tx_deadline_ms` clamps the hold to that boundary, which
        // amplifies rather than hides: a simulated 100 ms stale-tick defect, well inside
        // the old tolerance, surfaced as −13,390 ms and both bounds caught it. The gain
        // here is load-immunity, not reach.
        //
        // It still fails on the defect this test exists for. A raw `now_unix_ms()` re-read
        // puts the basis a whole OFFSET_MS past `after` — and past it either way, since the
        // unsteered key lands at an arbitrary phase and `tx_deadline_ms` may clamp it to the
        // boundary: 30 s out when it does not, ~32 s when it does.
        assert!(
            basis >= steered && basis <= after,
            "the hold must be measured on the STEERED clock: expected the basis inside the \
             window this call spanned ({steered:.0}..{after:.0}), got {basis:.0} — {:.0} ms \
             off. A raw now_unix_ms() re-read would be out by the whole {OFFSET_MS} ms offset.",
            basis - steered,
        );
    }

    /// Drive the async two-phase boundary synchronously (as the live loop does, but
    /// in-line instead of across the worker thread): decide, dispatch, run the
    /// decode, fold it, then run the TX phase. Returns the action + whether a
    /// boundary decode was actually folded. This is the exact ordering the loop
    /// preserves — decode of the just-ended slot ALWAYS folded before poll_tx.
    #[allow(clippy::too_many_arguments)]
    fn two_phase_boundary(
        eng: &mut Engine,
        rig: &mut Rig,
        backend: &mut MockBackend,
        rx: &mut RxRing,
        slot: u64,
        now_ms: f64,
        currently_tx: bool,
        prev_was_tx: bool,
    ) -> (SlotAction, bool) {
        // The live loop resyncs the capture epoch at every consumed boundary
        // (`Engine::begin_slot_capture`). This driver's ring is filled by the test
        // AFTER its fixture setup, so the capture belongs to the CURRENT context —
        // resync first, or a fixture `set_tier` (an epoch bump) would make the
        // boundary decode land stale (#103's guard misfiring on a fresh capture).
        eng.begin_slot_capture();
        if slot_wants_decode(currently_tx, prev_was_tx, rx.is_empty()) {
            let frame = rx.frame();
            // Phase 1: dispatch (build the owned job) + the heavy decode.
            let job = eng.build_decode_job(frame, slot, DecodePass::Boundary);
            let result = run_decode_job(job);
            // Phase 2a: fold the result under the engine lock.
            let (folded, rx_frame) = match eng.apply_decode_result(result) {
                DecodeApplied::Boundary { slot: _, frame, .. } => (true, Some(frame)),
                _ => (false, None),
            };
            // Phase 2b: the TX decision — now informed by the decode above.
            let act = slot_tx_phase(
                eng, rig, backend, rx, slot, now_ms, true, rx_frame, None, false,
            );
            (act, folded)
        } else {
            if prev_was_tx || currently_tx {
                rx.clear();
            }
            let act = slot_tx_phase(
                eng, rig, backend, rx, slot, now_ms, false, None, None, false,
            );
            (act, false)
        }
    }

    /// A logging rigctld stand-in: answers everything RPRT 0 and records the verb lines, so a
    /// test can assert on WHAT WENT ON THE WIRE rather than on a return value.
    fn logging_rigctld() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log_w = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut w = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let t = line.trim().to_string();
                    if t.is_empty() {
                        continue;
                    }
                    log_w.lock().unwrap().push(t);
                    let _ = w.write_all(b"RPRT 0\n");
                }
            }
        });
        (addr, log)
    }

    #[test]
    fn rig_split_puts_the_tx_vfo_in_the_operating_mode() {
        // Operator report (FTDX1200, then FTDX10): "working FT8 in split, the mode doesn't get
        // set for VFO B." It did not — `apply_tx_dial_shift` sent `S 1 VFOB` and `I <freq>` and
        // stopped, so VFO B transmitted in whatever mode it happened to be left in. The `M` verb
        // only ever reaches the RX VFO; `X` is the only way to reach the TX VFO's mode.
        //
        // WSJT-X is the baseline here and it does exactly this — HamlibTransceiver.cpp:922
        // `if (state().split()) rig_set_mode(rig, RIG_VFO_B, new_mode, RIG_PASSBAND_NOCHANGE)`
        // and :1093 `rig_set_split_mode(..., RIG_PASSBAND_NOCHANGE)`. Note the WIDTH: NOCHANGE
        // (-1), not an explicit passband — set the mode, leave the operator's filter alone.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::Ft8);
        let mut st = eng.settings().clone();
        st.split_mode = tempo_app::settings::SplitMode::Rig;
        eng.apply_settings(st);
        eng.set_tx_enabled(true);
        eng.set_tx_offset(750.0); // out of the 1500-2000 window → a real dial shift
        eng.broadcast("CQ W9XYZ EN37");

        let (addr, log) = logging_rigctld();
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        let md = eng.rig_mode_effective();
        // Through the real TX phase — the dial shift is PLANNED there, not by broadcast alone.
        let _ = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            None,
            None,
            false,
        );

        let sent = log.lock().unwrap().clone();
        assert!(
            sent.iter().any(|l| l.starts_with("S 1")),
            "precondition: rig split was engaged at all — wire was {sent:?}"
        );
        assert!(
            sent.iter().any(|l| l == &format!("X {md} -1")),
            "the TX VFO must be put in the operating mode, with the width left alone \
             (WSJT-X passes RIG_PASSBAND_NOCHANGE). wanted `X {md} -1`, wire was: {sent:?}"
        );
        // …and it must land AFTER split is on, or it addresses the wrong VFO.
        let i_split = sent
            .iter()
            .position(|l| l.starts_with("S 1"))
            .expect("split on");
        let i_mode = sent
            .iter()
            .position(|l| l.starts_with('X'))
            .expect("split mode");
        assert!(
            i_mode > i_split,
            "mode must be set after split is enabled: {sent:?}"
        );
    }

    #[test]
    fn fake_it_split_reports_the_restore_dial() {
        // FakeIt: an out-of-window TX offset shifts the dial for the over and
        // the action carries the dial to RESTORE once the over finishes — the
        // loop applies it after PTT drop — and the TX dial it moved to. Rig/None
        // report nothing to restore.
        // TX-only boundary (empty ring → no decode): the TX phase is called directly.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::Ft8);
        let mut st = eng.settings().clone();
        st.split_mode = tempo_app::settings::SplitMode::FakeIt;
        eng.apply_settings(st);
        eng.set_tx_enabled(true);
        eng.set_tx_offset(750.0); // f0 1750, dial -1000
        eng.broadcast("CQ W9XYZ EN37");
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            None,
            None,
            false,
        );

        assert!(act.tx_this_slot, "the CQ keyed");
        assert_eq!(
            act.fake_it_restore,
            Some(eng.settings().dial_hz()),
            "restore dial = the RX dial the over shifted away from"
        );
        assert_eq!(
            act.fake_it_tx_dial,
            Some(eng.settings().dial_hz() - 1000),
            "the TX dial = the RX dial with the over's -1000 Hz shift"
        );
    }

    // ── A split or Fake It dial write the radio does not take: nothing sent, TX halted ─────────────
    // WSJT-X writes the TX frequency before it asserts PTT (`Transceiver/TransceiverBase.cpp:160-180`,
    // `do_tx_frequency` ahead of `do_ptt(true)`), every Hamlib call through `error_check`, which
    // throws on anything but `RIG_OK` (`Transceiver/HamlibTransceiver.cpp:277-284`; the split writes
    // at `:1022`, `:1027`, `:1034`; Fake It's dial write in `EmulateSplitTransceiver.cpp:26-41`). The
    // throw skips `do_ptt(true)`, and the rig-failure handler presses Halt Tx
    // (`widgets/mainwindow.cpp:12556-12564`). Nexus's audio was already moved into 1500-2000 Hz for
    // the shifted dial, so keying on a dial that never moved puts the over 500 Hz or more away from
    // where the operator put it.

    /// A rigctld that answers each line with `answer(line)`: `Some(reply)` is written as it is, and
    /// `None` is never answered (the client's deadline passes). Every line logged. Each connection on
    /// a thread of its own, so a line left unanswered holds up nothing after the client reconnects.
    fn scripted_rigctld(
        answer: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log_w = log.clone();
        let answer = std::sync::Arc::new(answer);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (log, answer) = (log_w.clone(), answer.clone());
                std::thread::spawn(move || {
                    let mut w = stream.try_clone().unwrap();
                    for line in BufReader::new(stream).lines().map_while(Result::ok) {
                        let t = line.trim().to_string();
                        if t.is_empty() {
                            continue;
                        }
                        log.lock().unwrap().push(t.clone());
                        if let Some(reply) = answer(&t) {
                            if w.write_all(reply.as_bytes()).is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });
        (addr, log)
    }

    /// An FT8 CQ in Split Operation `split` whose 750 Hz TX offset needs a -1000 Hz dial shift, its
    /// clock on a slot start, keyed through the TX phase on a rigctld answering `answer`: the action,
    /// what played, the engine and every line the rigctld got.
    fn split_over_on(
        split: tempo_app::settings::SplitMode,
        answer: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> (SlotAction, MockBackend, Engine, Vec<String>) {
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::Ft8);
        let _ = eng.take_slot_tx_abort();
        let mut st = eng.settings().clone();
        st.split_mode = split;
        eng.apply_settings(st);
        eng.set_tx_enabled(true);
        eng.set_tx_offset(750.0);
        eng.broadcast("CQ TEST W9XYZ EN37");
        align_to_slot_start(&mut eng, 0);
        let (addr, log) = scripted_rigctld(answer);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        let now = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            now,
            false,
            None,
            None,
            false,
        );
        let sent = log.lock().unwrap().clone();
        (act, backend, eng, sent)
    }

    /// The split writes of one over, `verb` (`S`, `I`, `X` or `F`) answered `reply`, everything else
    /// `RPRT 0`; `None` leaves `verb` unanswered.
    fn one_split_write_answered(
        verb: &'static str,
        reply: Option<&'static str>,
    ) -> impl Fn(&str) -> Option<String> + Send + Sync + 'static {
        move |line: &str| {
            if line.starts_with(verb) {
                reply.map(str::to_string)
            } else {
                Some("RPRT 0\n".to_string())
            }
        }
    }

    /// What a split write the radio did not take must leave: the writes up to the one it did not
    /// take (`wire`) and nothing after, no key, nothing played, nothing held, TX off as WSJT-X's Halt
    /// Tx leaves it and the loop told to stop, and the reason on the refused key's line, as a split.
    fn assert_split_sent_nothing(
        what: &str,
        act: &SlotAction,
        backend: &MockBackend,
        eng: &mut Engine,
        sent: &[String],
        wire: &[&str],
    ) {
        assert_eq!(sent, wire, "{what}: on the wire");
        assert!(backend.played.is_empty(), "{what}: the over was played");
        assert!(
            !act.tx_this_slot && act.tx_until_ms.is_none(),
            "{what}: the over was taken as sent"
        );
        assert!(!eng.tx_enabled(), "{what}: TX is still on");
        assert!(
            eng.take_slot_tx_abort(),
            "{what}: the loop was not told to stop, as Stop TX tells it"
        );
        let refused = eng
            .snapshot()
            .radio
            .slot_key_refused
            .unwrap_or_else(|| panic!("{what}: nothing says why TX stopped"));
        assert!(
            serde_json::to_value(&refused).unwrap()["splitFailed"] == true,
            "{what}: not told as a split the radio did not take: {refused:?}"
        );
    }

    #[test]
    fn a_split_write_the_radio_refuses_sends_nothing_and_halts_tx_saying_why() {
        use tempo_app::settings::SplitMode;
        // Each of Rig split's three writes refused in turn (`S` and `I` with `RPRT -11`, Hamlib's
        // "not available", what Nexus's own Flex client answers them with; `X` with `RPRT -1`), and
        // Fake It's dial write refused (`RPRT -9`, an Icom's NG): the writes stop at the refusal,
        // as WSJT-X's throw does.
        for (split, verb, reply, wire) in [
            (SplitMode::Rig, "S ", "RPRT -11\n", &["S 1 VFOB"][..]),
            (
                SplitMode::Rig,
                "I ",
                "RPRT -11\n",
                &["S 1 VFOB", "I 14073000"][..],
            ),
            (
                SplitMode::Rig,
                "X ",
                "RPRT -1\n",
                &["S 1 VFOB", "I 14073000", "X PKTUSB -1"][..],
            ),
            (SplitMode::FakeIt, "F ", "RPRT -9\n", &["F 14073000"][..]),
        ] {
            let what = format!("{split:?}, {verb:?} answered {reply:?}");
            let (act, backend, mut eng, sent) =
                split_over_on(split, one_split_write_answered(verb, Some(reply)));
            assert_split_sent_nothing(&what, &act, &backend, &mut eng, &sent, wire);
            // The radio is put back as after any over: Rig split released by the loop's teardown,
            // Fake It's dial written back.
            match split {
                SplitMode::Rig => assert!(act.rig_split_engaged, "{what}: the split not released"),
                _ => assert_eq!(
                    act.fake_it_restore,
                    Some(eng.settings().dial_hz()),
                    "{what}: the dial not written back"
                ),
            }
        }
    }

    #[test]
    fn a_split_write_the_radio_does_not_answer_sends_nothing_and_halts_tx() {
        use tempo_app::settings::SplitMode;
        // No answer by the CAT deadline, and an empty reply (a bare newline, no `RPRT`), on each
        // write: an answer that never says `RPRT 0` is not one the radio took.
        for (split, verb, reply, wire) in [
            (SplitMode::Rig, "S ", None, &["S 1 VFOB"][..]),
            (SplitMode::Rig, "I ", None, &["S 1 VFOB", "I 14073000"][..]),
            (
                SplitMode::Rig,
                "X ",
                None,
                &["S 1 VFOB", "I 14073000", "X PKTUSB -1"][..],
            ),
            (SplitMode::FakeIt, "F ", None, &["F 14073000"][..]),
            (
                SplitMode::Rig,
                "I ",
                Some("\n"),
                &["S 1 VFOB", "I 14073000"][..],
            ),
            (SplitMode::FakeIt, "F ", Some("\n"), &["F 14073000"][..]),
        ] {
            let what = format!("{split:?}, {verb:?} answered {reply:?}");
            let (act, backend, mut eng, sent) =
                split_over_on(split, one_split_write_answered(verb, reply));
            assert_split_sent_nothing(&what, &act, &backend, &mut eng, &sent, wire);
        }
    }

    #[test]
    fn a_split_the_radio_takes_keys_and_plays_its_over_as_before() {
        use tempo_app::settings::SplitMode;
        // The control: every write answered `RPRT 0`. The split, the key, the over played, TX on,
        // nothing on the refused key's line. And a link that cannot set the TX VFO's mode at all
        // (`X` answered `RPRT -11`, as Nexus's own TCI client answers it) with its TX dial taken.
        let x_not_available = |line: &str| {
            Some(
                if line.starts_with("X ") {
                    "RPRT -11\n"
                } else {
                    "RPRT 0\n"
                }
                .to_string(),
            )
        };
        let (act, backend, eng, sent) = split_over_on(SplitMode::Rig, x_not_available);
        assert_eq!(
            sent,
            ["S 1 VFOB", "I 14073000", "X PKTUSB -1", "T 1"],
            "X not available: on the wire"
        );
        assert!(
            act.tx_this_slot && !backend.played.is_empty() && eng.tx_enabled(),
            "X not available: not keyed and played"
        );
        for (split, wire) in [
            (
                SplitMode::Rig,
                &["S 1 VFOB", "I 14073000", "X PKTUSB -1", "T 1"][..],
            ),
            (SplitMode::FakeIt, &["F 14073000", "T 1"][..]),
        ] {
            let what = format!("{split:?}");
            let (act, backend, eng, sent) =
                split_over_on(split, |_: &str| Some("RPRT 0\n".to_string()));
            assert_eq!(sent, wire, "{what}: on the wire");
            assert!(
                act.tx_this_slot && !backend.played.is_empty(),
                "{what}: not keyed and played"
            );
            assert!(eng.tx_enabled(), "{what}: TX went off");
            assert_eq!(eng.snapshot().radio.slot_key_refused, None, "{what}");
        }
    }

    #[test]
    fn tx_slot_keys_ptt_plays_audio_and_sets_hold() {
        // Engine with tx_parity 0 transmits on EVEN slots; queue a broadcast.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        eng.broadcast("CQ TEST W9XYZ EN37");
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        // No captured RX (empty ring) → the TX phase runs directly.
        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            false,
        );

        assert!(!folded, "empty ring → no decode folded");
        assert!(rig.keyed, "PTT keyed for the TX over");
        assert!(
            !backend.played.is_empty(),
            "transmit audio played to the backend"
        );
        assert!(
            act.tx_until_ms.unwrap() > 1000.0 + 250.0,
            "PTT held for audio duration + tail"
        );
        assert!(!act.did_rx);
        assert!(act.tx_this_slot, "flagged as a transmit slot");
    }

    #[test]
    fn rx_slot_decodes_without_keying() {
        // Idle engine → nothing to send even on its TX slot → receive path. The
        // two-phase driver decodes the captured slot, folds it, THEN runs poll_tx.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::TempoFast); // FT1-modem slot test (default is FT8)
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]); // a captured RX slot sits in the ring

        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            false,
        );

        assert!(folded, "the RX frame was decoded + folded before poll_tx");
        assert!(!rig.keyed, "no PTT on a receive slot");
        assert!(backend.played.is_empty(), "no audio played on RX");
        assert!(act.did_rx, "reported as a decode slot");
        assert!(!act.tx_this_slot);
        assert!(act.tx_until_ms.is_none());
    }

    #[test]
    fn mid_transmit_does_not_double_decode() {
        // While the PTT tail is still held (currently_tx), an idle slot is a no-op:
        // we must NOT decode (we'd be decoding our own tail) and not re-key.
        assert!(
            !slot_wants_decode(true, false, false),
            "a TX tail crossing the boundary suppresses the decode"
        );
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            true,
            false,
        );

        assert!(!folded, "no RX decode mid-transmit");
        assert!(!act.did_rx);
        assert!(act.tx_until_ms.is_none());
        assert!(!rig.keyed);
    }

    #[test]
    fn rx_slot_between_transmits_is_decoded() {
        // The regression: calling CQ (TX on even slots), the RX slot's captured
        // audio must be decoded at the next (TX) boundary — BEFORE we re-key — not
        // cleared away unheard. prev_was_tx=false means the slot that just ended was
        // a receive slot, so its audio (in the ring) is the other stations.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        eng.set_tier(tempo_app::dto::Tier::TempoFast);
        eng.broadcast("CQ TEST W9XYZ EN37"); // something to send on our TX slot
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]); // the RX slot we just finished, captured

        // Even (TX) slot boundary, prior slot was RX (prev_was_tx=false).
        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            2,
            1000.0,
            false,
            false,
        );

        assert!(
            folded,
            "the RX slot's audio is decoded + folded before we transmit again"
        );
        assert!(act.tx_this_slot, "and then we send our CQ");
        assert!(rig.keyed, "PTT keyed for the CQ over");
    }

    #[test]
    fn own_transmit_slot_is_not_decoded_as_rx() {
        // After we transmitted (prev_was_tx=true) the ring holds our own carrier —
        // it must NOT be decoded, even though it is non-empty.
        assert!(
            !slot_wants_decode(false, true, false),
            "our own transmit slot is never decoded as RX"
        );
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::TempoFast);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]); // our own transmission's captured audio

        // Odd (RX) slot boundary; the slot that just ended was our TX.
        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            1,
            1000.0,
            false,
            true,
        );

        assert!(!folded, "must not decode our own transmission");
        assert!(!act.did_rx);
        assert!(!act.tx_this_slot);
        assert!(rx.is_empty(), "own-carrier ring is cleared, not decoded");
    }

    #[test]
    fn run_slot_matches_two_phase_for_a_receive_slot() {
        // The synchronous `run_slot` reference and the two-phase decomposition must
        // agree: both decode the just-ended RX slot before the (no-op) TX decision.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::TempoFast);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]);
        let act = run_slot(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            false,
        );
        assert!(act.did_rx, "run_slot decodes the RX slot");
        assert!(!act.tx_this_slot);
    }

    // ── A key the radio does not accept: WSJT-X's rig failure ──────────────────────────────
    // WSJT-X 3.0.2 starts an over's audio only once the rig reports PTT on (mainwindow.cpp:
    // 12440-12448, "safe to start audio"), and a PTT Hamlib answers with ANY error code, its own
    // timeouts included, is a rig failure that presses Halt Tx (:12556-12564). It sets no deadline
    // of its own on the key: a slow rig keys late and its audio follows.

    /// What [`keying_rigctld`] hands back: its address, every line it was sent, and the instant it
    /// answered the key.
    type KeyingRigctld = (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    );

    /// A rigctld that answers the key (`T 1`) with `key` after `delay_ms`, and everything else
    /// with `RPRT 0` at once. Logs each line as it arrives, and the instant the key's answer was
    /// written.
    fn keying_rigctld(key: &str, delay_ms: u64) -> KeyingRigctld {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let answered = std::sync::Arc::new(std::sync::Mutex::new(None));
        let (log_w, answered_w) = (log.clone(), answered.clone());
        let key = key.to_string();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut w = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let t = line.trim().to_string();
                    if t.is_empty() {
                        continue;
                    }
                    log_w.lock().unwrap().push(t.clone());
                    let reply = if t == "T 1" {
                        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                        *answered_w.lock().unwrap() = Some(std::time::Instant::now());
                        format!("{key}\n")
                    } else {
                        "RPRT 0\n".to_string()
                    };
                    if w.write_all(reply.as_bytes()).is_err() {
                        return;
                    }
                }
            }
        });
        (addr, log, answered)
    }

    /// A stand-in over shaped like FT8's: 0.5 s of lead-in, then 12.64 s of a constant tone. Slot-
    /// positioned like the real one, so an FT8 key that comes a little late (the key's own round
    /// trip, a busy machine) still has its tones ahead of the slot clock, as a real over does.
    fn stand_in_over() -> Vec<f32> {
        let mut wave = vec![0.0; 6_000];
        wave.resize(FT8_WAVE, 0.25);
        wave
    }

    /// One over through the TX phase, as the boundary keys it with nothing else holding the
    /// transmitter, on a waveform built beforehand so only the key and the play are measured.
    fn key_one_over(
        eng: &mut Engine,
        rig: &mut Rig,
        backend: &mut impl AudioBackend,
    ) -> SlotAction {
        let mut rx = RxRing::new();
        let now = steered_now_ms(eng);
        slot_tx_phase(
            eng,
            rig,
            backend,
            &mut rx,
            0,
            now,
            false,
            None,
            Some(vec![stand_in_over()]),
            false,
        )
    }

    /// What a refused key must leave: nothing played, nothing held, TX off as WSJT-X's Halt Tx
    /// leaves it, the loop told to cut and unkey as Stop TX tells it, and the reason kept.
    fn assert_sent_nothing_and_halted(
        what: &str,
        eng: &mut Engine,
        act: &SlotAction,
        backend: &MockBackend,
        log: &std::sync::Mutex<Vec<String>>,
    ) {
        assert!(
            log.lock().unwrap().iter().any(|l| l == "T 1"),
            "{what}: premise: the over tried the key"
        );
        assert!(
            backend.played.is_empty(),
            "{what}: the over was played into a radio that did not key"
        );
        assert!(
            !act.tx_this_slot && act.tx_until_ms.is_none(),
            "{what}: the over was taken as sent"
        );
        assert!(
            !eng.tx_enabled(),
            "{what}: TX is still on (WSJT-X's Halt Tx unticks Enable Tx)"
        );
        assert!(
            eng.take_slot_tx_abort(),
            "{what}: the loop was not told to cut and unkey, as Stop TX tells it"
        );
        assert!(
            eng.snapshot().radio.slot_key_refused.is_some(),
            "{what}: nothing says why TX stopped"
        );
    }

    #[test]
    fn a_key_the_radio_refuses_sends_nothing_and_halts_tx_as_wsjtx_does() {
        // ⭐ THE DEFECT. Every slot over keyed with `let _ = rig.ptt(true)` and played whatever
        // the answer, so a radio that said no got the whole over played into it while it was
        // receiving, and TX stayed armed to do it again next cycle. WSJT-X plays nothing (its
        // audio waits for PTT on) and halts.
        for (tier, answer) in [
            (tempo_app::dto::Tier::Ft8, "RPRT -1"),
            (tempo_app::dto::Tier::Ft8, "RPRT -9"),
            (tempo_app::dto::Tier::Ft4, "RPRT -1"),
        ] {
            let what = format!("{tier:?} {answer}");
            let mut eng = Engine::new("W9XYZ", "EN37", 0);
            eng.set_tier(tier);
            eng.set_tx_enabled(true);
            // On a slot boundary: FT8 and FT4 lay their tones on the slot clock, so a key at a
            // random phase past the last tone would key nothing at all (see `armed_engine`).
            align_to_slot_start(&mut eng, 0);
            assert!(eng.tx_enabled(), "{what}: premise: TX is armed");
            let (addr, log, _) = keying_rigctld(answer, 0);
            let mut rig = Rig::rigctld(&addr);
            let mut backend = MockBackend::new();
            let act = key_one_over(&mut eng, &mut rig, &mut backend);
            assert_sent_nothing_and_halted(&what, &mut eng, &act, &backend, &log);
            let why = eng.snapshot().radio.slot_key_refused.unwrap().why;
            assert!(why.contains(answer), "{what}: the rig's own answer: {why}");
        }

        // The control, in the same scene: a radio that keys gets its over, and TX stays on.
        let mut eng = armed_engine();
        let (addr, _log, _) = keying_rigctld("RPRT 0", 0);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let act = key_one_over(&mut eng, &mut rig, &mut backend);
        assert!(
            act.tx_this_slot && !backend.played.is_empty(),
            "control: no over"
        );
        assert!(eng.tx_enabled(), "control: TX went off");
        assert!(!eng.take_slot_tx_abort(), "control: a cut was asked for");
        assert_eq!(eng.snapshot().radio.slot_key_refused, None, "control");
    }

    #[test]
    fn hamlibs_own_no_answer_to_a_key_halts_tx_as_wsjtx_does() {
        // ⭐ HERE A SLOT OVER DIFFERS FROM THE OTHER MODES, ON PURPOSE. rigctld answers a key it
        // could not get the radio to answer with Hamlib's own code (`RPRT -5` timeout, `-6` I/O,
        // `-13` bus error, `-14` bus busy). The voice keyer, RTTY and the rest play on it, with
        // their warning (`service::KeyUp::NoAnswer`). WSJT-X's `error_check` throws on every code
        // but `RIG_OK` (HamlibTransceiver.cpp:277-284), so for it this is a rig failure like any
        // refusal, and the slot overs follow WSJT-X.
        for code in [-5, -6, -13, -14] {
            let what = format!("RPRT {code}");
            let mut eng = armed_engine();
            assert!(eng.tx_enabled(), "{what}: premise: TX is armed");
            let (addr, log, _) = keying_rigctld(&what, 0);
            let mut rig = Rig::rigctld(&addr);
            let mut backend = MockBackend::new();
            let act = key_one_over(&mut eng, &mut rig, &mut backend);
            assert_sent_nothing_and_halted(&what, &mut eng, &act, &backend, &log);
        }
    }

    #[test]
    fn a_late_answer_to_a_key_still_plays_the_over() {
        // Nexus's own PTT deadline is the one failure WSJT-X has no counterpart for: it waits on
        // Hamlib, and a slow rig keys late and plays. A slow radio behind rigctld (a Xiegu, a
        // vintage Kenwood, any rig at 19200 baud or less) answers after the deadline, so its over
        // still goes out here, as it always did. 1.5 s, because the deadline is looked at only
        // between 500 ms reads (an answer inside the first second is still taken as keyed).
        let mut eng = armed_engine();
        let (addr, log, _) = keying_rigctld("RPRT 0", 1_500);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let act = key_one_over(&mut eng, &mut rig, &mut backend);
        assert!(
            log.lock().unwrap().iter().any(|l| l == "T 1"),
            "premise: the over tried the key"
        );
        assert!(
            act.tx_this_slot && !backend.played.is_empty(),
            "a late answer dropped the over"
        );
        assert!(eng.tx_enabled(), "a late answer turned TX off");
        assert!(!eng.take_slot_tx_abort(), "a late answer asked for a cut");
        assert_eq!(eng.snapshot().radio.slot_key_refused, None);
        assert!(
            rig.keyed,
            "the radio may be keying: PTT stays asserted for the over"
        );
    }

    #[test]
    fn a_failed_key_while_the_loop_holds_the_transmitter_still_plays() {
        // A key while the loop already holds the transmitter is not a new one. Nexus's own Flex
        // client refuses every key after the first while one of ours is held (`AlreadyKeyed`), and
        // WSJT-X never sends one (it asks for PTT only on a change). The over goes out on the held
        // key, as it always did, and as the other modes' `service::KeyUp::Held` does.
        let mut eng = armed_engine();
        let (addr, log, _) = keying_rigctld("RPRT -1", 0);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        let now = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            now,
            false,
            None,
            Some(vec![stand_in_over()]),
            true,
        );
        assert!(
            log.lock().unwrap().iter().any(|l| l == "T 1"),
            "premise: the over tried the key"
        );
        assert!(
            act.tx_this_slot && !backend.played.is_empty(),
            "a key refused while held dropped the over"
        );
        assert!(eng.tx_enabled(), "a key refused while held turned TX off");
        assert!(!eng.take_slot_tx_abort());
        assert_eq!(eng.snapshot().radio.slot_key_refused, None);
    }

    #[test]
    fn a_slot_over_whose_audio_output_will_not_open_never_keys_and_halts_tx() {
        // ⭐ NEVER KEY THE RADIO WITH NO AUDIO. The sound card's output used to be opened by the
        // over's own `play`, after the key, so an output that would not open put a dead carrier on
        // the air for the whole over, and every over after it. The radio loop now opens it when
        // TX is armed; this is the backstop at the key itself, for every slot keying site: no PTT
        // command on the wire, nothing played, TX halted, and the reason on the refused key's line.
        const NO_OUTPUT: &str = "playback stream: Device or resource busy";
        for tier in [tempo_app::dto::Tier::Ft8, tempo_app::dto::Tier::Ft4] {
            let what = format!("{tier:?}");
            let mut eng = Engine::new("W9XYZ", "EN37", 0);
            eng.set_tier(tier);
            eng.set_tx_enabled(true);
            // On a slot boundary: FT8 and FT4 lay their tones on the slot clock, so a key at a
            // random phase past the last tone would key nothing at all (see `armed_engine`).
            align_to_slot_start(&mut eng, 0);
            let (addr, log, _) = keying_rigctld("RPRT 0", 0);
            let mut rig = Rig::rigctld(&addr);
            let mut backend = MockBackend::new();
            backend.tx_open_error = Some(NO_OUTPUT.to_string());
            let act = key_one_over(&mut eng, &mut rig, &mut backend);
            let sent = log.lock().unwrap().clone();
            assert!(
                sent.is_empty(),
                "{what}: a command went on the wire: {sent:?}"
            );
            assert!(!rig.keyed, "{what}: PTT asserted");
            assert!(backend.played.is_empty(), "{what}: the over was played");
            assert!(
                !act.tx_this_slot && act.tx_until_ms.is_none(),
                "{what}: the over was taken as sent"
            );
            assert!(!eng.tx_enabled(), "{what}: TX is still on");
            assert!(
                eng.take_slot_tx_abort(),
                "{what}: the loop was not told to stop, as Stop TX tells it"
            );
            let refused = eng
                .snapshot()
                .radio
                .slot_key_refused
                .unwrap_or_else(|| panic!("{what}: nothing says why TX stopped"));
            assert!(
                refused.tx_audio_failed,
                "{what}: told as a radio refusal: {refused:?}"
            );
            assert_eq!(refused.why, NO_OUTPUT, "{what}: in the sound card's words");
        }

        // The control, in the same scene: the same over on an output that opens keys and plays.
        let mut eng = armed_engine();
        let (addr, log, _) = keying_rigctld("RPRT 0", 0);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let act = key_one_over(&mut eng, &mut rig, &mut backend);
        assert_eq!(*log.lock().unwrap(), ["T 1"], "control: the key");
        assert!(
            act.tx_this_slot && !backend.played.is_empty(),
            "control: no over"
        );
        assert!(eng.tx_enabled(), "control: TX went off");
        assert_eq!(eng.snapshot().radio.slot_key_refused, None, "control");
    }

    /// A backend that notes when it was first handed audio.
    #[derive(Default)]
    struct StampedBackend {
        played: usize,
        first_play: Option<std::time::Instant>,
    }

    impl AudioBackend for StampedBackend {
        fn capture(&mut self) -> Vec<f32> {
            Vec::new()
        }
        fn play(&mut self, samples: &[f32]) {
            self.first_play.get_or_insert_with(std::time::Instant::now);
            self.played += samples.len();
        }
    }

    #[test]
    fn a_keyed_over_starts_its_audio_the_moment_the_key_is_answered() {
        // ⏱ THE POSITIVE CONTROL FOR THE REFUSED-KEY STOP: on a key the radio accepts, nothing
        // new runs before the key or between its answer and the audio. The key is the first
        // line on the wire and the only one, and the audio starts after the answer, never
        // before it (WSJT-X's own order), with no CAT exchange or wait in between. Ordering
        // assertions plus a bound under one PTT deadline, not a tolerance on machine speed: the
        // measured times are printed for the record (`--nocapture`).
        const KEY_MS: u64 = 200;
        let mut answered_to_play = Vec::new();
        let mut call_to_answer = Vec::new();
        for _ in 0..5 {
            // FT2: FT8 and FT4 lay a keyed over on the slot clock and play only what is still ahead
            // of it, so the whole over plays from the key only on the other tiers; the key's rule
            // pinned here is every slot tier's.
            let mut eng = armed_engine_on(tempo_app::dto::Tier::Ft2);
            let (addr, log, answered) = keying_rigctld("RPRT 0", KEY_MS);
            let mut rig = Rig::rigctld(&addr);
            let mut backend = StampedBackend::default();
            let called = std::time::Instant::now();
            let act = key_one_over(&mut eng, &mut rig, &mut backend);
            let returned = std::time::Instant::now();
            assert!(
                act.tx_this_slot && backend.played == FT8_WAVE,
                "the over keyed"
            );
            assert_eq!(*log.lock().unwrap(), ["T 1"], "the key, and only the key");
            let answered = answered.lock().unwrap().expect("the key was answered");
            let first_play = backend.first_play.expect("the audio was played");
            assert!(first_play >= answered, "audio before the key's answer");
            assert!(first_play <= returned);
            let gap = first_play - answered;
            assert!(
                gap < std::time::Duration::from_millis(500),
                "{gap:?} between the key's answer and the audio: something now waits there"
            );
            answered_to_play.push(gap.as_secs_f64() * 1000.0);
            call_to_answer.push((answered - called).as_secs_f64() * 1000.0);
        }
        println!(
            "keyed over: call to answer {call_to_answer:.2?} ms (the key itself is {KEY_MS} ms), \
             answer to audio {answered_to_play:.3?} ms"
        );
    }
}
