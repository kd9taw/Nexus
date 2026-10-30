//! Receive decoders that run because their SCREEN is open — and stop when the operator leaves it.
//!
//! Opening the RTTY, PSK, JTTY, APRS or SSTV screen starts that mode's decoder (`*_auto_arm`):
//! a receive screen with a dead receiver is the field bug those arms exist to fix. Nothing ever
//! stopped them again, so an operator who looked at all five and went back to FT8 left five
//! decoders running on the receive audio for the rest of the session (several percent of a core
//! together; SSTV alone runs an FFT every 10 ms on silence).
//!
//! [`Engine::leave_decoder_screen`] is the other half: the UI calls it when the operator (at the
//! station, through the Remote picture, or from a computer driving it) moves to ANOTHER SECTION.
//! A glance at the map or the logbook is not a section change and stops nothing.
//!
//! What it stops, and what it never does:
//! - ONLY a decoder its screen started. An explicit Arm (the cockpit's button, a manual SSTV
//!   start, the ISS pass arm) clears the `*_auto_armed` mark, so a decoder the operator started
//!   keeps running wherever they go.
//! - It is an AUTOMATIC stop, so it does NOT latch `*_auto_arm_declined` (the `sstv_auto_disarm`
//!   rule): opening the screen again starts the decoder again, and the transcript, receive
//!   window, heard list and gallery are all still there.
//! - A decoder something else still needs stays on: the RTTY auto-sequencer reads the decode
//!   to decide its next over, the APRS iGate contributes what the decoder hears, and a screen
//!   still showing in its own window (the SSTV picture viewer) is still that section.
//!
//! RX ONLY. Disarming a decoder keys nothing and unkeys nothing; every transmit path of these
//! modes is separate and untouched (the APRS auto-ack needs an EXPLICIT arm, which this never
//! stops).

use super::{AprsArm, AprsHealth, Engine};
use std::time::{Duration, Instant};

/// How long the station waits, once no computer is attached, before carrying out the leaves held
/// while one was: two of presence's 5 s windows. A gap that short is a blip (a stalled heartbeat,
/// a throttled client window, a reconnect), not the computer going, and stopping its viewer's
/// receiver on it would lose the pictures the hold exists to keep. Receive only, so waiting costs
/// only a few seconds of decoding.
pub const DETACH_GRACE: Duration = Duration::from_secs(10);

/// A receive screen whose decoder starts when the operator opens it. "rtty", "psk", "jtty",
/// "sstv", "aprs" on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecoderScreen {
    Rtty,
    Psk,
    Jtty,
    Sstv,
    Aprs,
}

impl Engine {
    /// The SSTV picture viewer's window is open at the station (`open_panel_window` and the
    /// window's `Destroyed` event keep this, and `leave_decoder_screen` refreshes it): it shows
    /// the SSTV screen's pictures, so the screen is still in the section.
    pub fn set_sstv_viewer_open(&mut self, open: bool) {
        self.sstv_viewer_open = open;
    }

    /// The operator left `screen` for another section. Stops its decoder if, and only if, opening
    /// the screen is what started it and nothing else still needs it.
    ///
    /// Returns whether the decoder is STILL running on its screen's arm — kept for one of the
    /// reasons above — so the caller can ask again at the next section change. A decoder the
    /// operator armed, or one already off, returns false: there is nothing left to ask about.
    pub fn leave_decoder_screen(&mut self, screen: DecoderScreen) -> bool {
        let auto_armed = match screen {
            DecoderScreen::Rtty => self.rtty_armed && self.rtty_auto_armed,
            DecoderScreen::Psk => self.psk_armed && self.psk_auto_armed,
            DecoderScreen::Jtty => self.jtty_auto_armed(),
            DecoderScreen::Sstv => self.sstv_armed && self.sstv_auto_armed,
            DecoderScreen::Aprs => self.aprs_arm == AprsArm::Auto && self.aprs_auto_armed,
        };
        if !auto_armed {
            return false;
        }
        // A computer driving the station is attached: its screen and the shack window's each send
        // their own leaves against this one engine, and neither knows what the other shows. Stop
        // nothing (a decoder left on is the safe side: receive only) and carry the leave out once
        // that computer has gone ([`Self::carry_out_held_leaves`]).
        if self.computer_attached(Instant::now()) {
            if !self.decoder_leaves_held.contains(&screen) {
                self.decoder_leaves_held.push(screen);
            }
            return true;
        }
        let still_needed = match screen {
            // The auto-sequencer reads the decode to pick its next over.
            DecoderScreen::Rtty => self.rtty_seq.is_some(),
            // The RX iGate contributes what this decoder hears.
            DecoderScreen::Aprs => self.settings.aprs_is_uplink,
            // The picture viewer, open in its own window, is still the SSTV screen.
            DecoderScreen::Sstv => self.sstv_viewer_open,
            DecoderScreen::Psk | DecoderScreen::Jtty => false,
        };
        if still_needed {
            return true;
        }
        // Each stop mirrors its mode's own disarm, without the decline memory.
        match screen {
            DecoderScreen::Rtty => {
                self.rtty_audio.clear();
                self.rtty_scope = super::RttyScope::default();
                self.rtty_auto_armed = false;
                self.rtty_armed = false;
            }
            DecoderScreen::Psk => {
                self.psk_audio.clear();
                self.psk_auto_armed = false;
                self.psk_armed = false;
            }
            DecoderScreen::Jtty => self.jtty_auto_disarm(),
            DecoderScreen::Sstv => self.sstv_auto_disarm(),
            DecoderScreen::Aprs => {
                self.aprs_audio.clear();
                self.aprs_health = AprsHealth::default();
                self.aprs_auto_armed = false;
                self.aprs_arm = AprsArm::Off;
            }
        }
        false
    }

    /// A computer is attached to the station: it holds live presence (its Stop road's heartbeat, or
    /// a streamed controller), OR it holds the station lease without presence (on the internet with
    /// datagrams blocked, or a LAN whose Stop road is not heard). Either way its screens send leaves
    /// and its picture viewer watches the shack's receiver. The lease is read off the radio loop's
    /// authority poll (`poll_remote_transmit`).
    fn computer_attached(&self, now: Instant) -> bool {
        self.remote_presence_live(now) || self.remote_lease_held
    }

    /// Opening `screen` again cancels a leave held for it: it is in front once more.
    pub(super) fn decoder_screen_entered(&mut self, screen: DecoderScreen) {
        self.decoder_leaves_held.retain(|held| *held != screen);
    }

    /// Carry out the leaves held while a computer was attached, once none has been for
    /// [`DETACH_GRACE`]: each decoder
    /// still running on its screen's arm stops, unless something still needs it (that one stays
    /// held and is asked again at the next poll). Run on the radio loop's poll
    /// (`poll_remote_stream`); with nothing held it costs one check.
    pub(super) fn carry_out_held_leaves(&mut self, now: Instant) {
        if self.decoder_leaves_held.is_empty() {
            self.decoders_detached_since = None;
            return;
        }
        if self.computer_attached(now) {
            self.decoders_detached_since = None;
            return;
        }
        // Gone only once it has stayed gone for the grace (a blip is not the computer going).
        let since = *self.decoders_detached_since.get_or_insert(now);
        if now.saturating_duration_since(since) < DETACH_GRACE {
            return;
        }
        for screen in std::mem::take(&mut self.decoder_leaves_held) {
            if self.leave_decoder_screen(screen) {
                self.decoder_leaves_held.push(screen);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::remote_transmit::RemoteTxHold;
    use super::*;
    use crate::remote_control::transmit::TransmitAuthority;
    use crate::settings::Settings;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    const SCREENS: [DecoderScreen; 5] = [
        DecoderScreen::Rtty,
        DecoderScreen::Psk,
        DecoderScreen::Jtty,
        DecoderScreen::Sstv,
        DecoderScreen::Aprs,
    ];

    fn auto_arm(e: &mut Engine, s: DecoderScreen) -> bool {
        match s {
            DecoderScreen::Rtty => e.rtty_auto_arm(),
            DecoderScreen::Psk => e.psk_auto_arm(),
            DecoderScreen::Jtty => e.jtty_auto_arm(),
            DecoderScreen::Sstv => e.sstv_auto_arm(),
            DecoderScreen::Aprs => e.aprs_auto_arm(),
        }
    }

    /// The operator's own Arm, as each cockpit's button sends it.
    fn operator_arm(e: &mut Engine, s: DecoderScreen) {
        match s {
            DecoderScreen::Rtty => e.set_rtty_armed(true),
            DecoderScreen::Psk => e.set_psk_armed(true),
            DecoderScreen::Jtty => e.set_jtty_armed(true),
            DecoderScreen::Sstv => e.set_sstv_armed(true),
            DecoderScreen::Aprs => e.set_aprs_arm(AprsArm::Explicit),
        }
    }

    fn armed(e: &Engine, s: DecoderScreen) -> bool {
        match s {
            DecoderScreen::Rtty => e.rtty_armed(),
            DecoderScreen::Psk => e.psk_armed(),
            DecoderScreen::Jtty => e.jtty_armed(),
            DecoderScreen::Sstv => e.sstv_armed(),
            DecoderScreen::Aprs => e.aprs_armed(),
        }
    }

    /// ⭐ THE FIX. A decoder its screen started stops when the operator leaves for another
    /// section — every one of the five.
    #[test]
    fn a_decoder_its_screen_started_stops_when_the_screen_is_left() {
        for s in SCREENS {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            assert!(auto_arm(&mut e, s), "{s:?}: opening the screen arms");
            assert!(!e.leave_decoder_screen(s), "{s:?}: nothing keeps it");
            assert!(!armed(&e, s), "{s:?}: leaving stopped the decoder");
        }
    }

    /// Leaving stops nothing ELSE: only the screen named.
    #[test]
    fn leaving_one_screen_leaves_the_other_decoders_alone() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        for s in SCREENS {
            assert!(auto_arm(&mut e, s));
        }
        e.leave_decoder_screen(DecoderScreen::Sstv);
        assert!(!e.sstv_armed(), "the screen left");
        for s in SCREENS.into_iter().filter(|s| *s != DecoderScreen::Sstv) {
            assert!(armed(&e, s), "{s:?} was not left and keeps running");
        }
    }

    /// A decoder the OPERATOR armed is theirs: leaving the screen does not stop it — including
    /// one the screen had started first and the operator then took over (a manual SSTV start,
    /// the ISS pass arm).
    #[test]
    fn a_decoder_the_operator_armed_keeps_running_when_the_screen_is_left() {
        for s in SCREENS {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            operator_arm(&mut e, s);
            assert!(!e.leave_decoder_screen(s), "{s:?}: nothing to ask");
            assert!(armed(&e, s), "{s:?}: the operator's arm survives");

            let mut f = Engine::new("W9XYZ", "EN61", 0);
            assert!(auto_arm(&mut f, s));
            operator_arm(&mut f, s);
            f.leave_decoder_screen(s);
            assert!(
                armed(&f, s),
                "{s:?}: an explicit arm after entry is the operator's"
            );
        }
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.sstv_auto_arm());
        e.request_sstv_manual_rx("pd120".to_string());
        e.leave_decoder_screen(DecoderScreen::Sstv);
        assert!(e.sstv_armed(), "a manual receive start is the operator's");
        assert_eq!(e.take_sstv_manual_rx().as_deref(), Some("pd120"));
    }

    /// ⭐ AN AUTOMATIC STOP IS NOT THE OPERATOR'S STOP. Coming back to the screen starts the
    /// decoder again, and what it had decoded is still there.
    #[test]
    fn coming_back_to_the_screen_starts_the_decoder_again() {
        for s in SCREENS {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            assert!(auto_arm(&mut e, s));
            e.leave_decoder_screen(s);
            assert!(auto_arm(&mut e, s), "{s:?}: re-entry is not refused");
            assert!(armed(&e, s), "{s:?}: running again");
            // …and it is still the screen's arm, so the next leave stops it again.
            e.leave_decoder_screen(s);
            assert!(!armed(&e, s), "{s:?}: the second leave stops it too");
        }
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.rtty_auto_arm() && e.psk_auto_arm());
        let ch = |ch| tempo_core::textmode::DecodedChar {
            ch,
            confidence: 1.0,
        };
        e.push_rtty_decode(&[ch('R'), ch('Y')], 0.0, false);
        e.push_psk_decode(&[ch('C'), ch('Q')], 0.0, true);
        e.leave_decoder_screen(DecoderScreen::Rtty);
        e.leave_decoder_screen(DecoderScreen::Psk);
        assert!(e.rtty_auto_arm() && e.psk_auto_arm());
        assert_eq!(
            e.rtty_state().text,
            "RY",
            "the RTTY transcript survives the round trip"
        );
        assert_eq!(
            e.psk_state().text,
            "CQ",
            "the PSK transcript survives the round trip"
        );
    }

    /// The operator's own Stop is still remembered across a leave (the decline memory is
    /// untouched), and leaving a screen whose decoder is off changes nothing.
    #[test]
    fn an_operator_stop_stays_remembered_and_an_idle_leave_does_nothing() {
        for s in SCREENS {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            assert!(!e.leave_decoder_screen(s), "{s:?}: off, nothing to do");
            assert!(
                auto_arm(&mut e, s),
                "{s:?}: an idle leave latched no decline"
            );
            let mut f = Engine::new("W9XYZ", "EN61", 0);
            operator_arm(&mut f, s);
            match s {
                DecoderScreen::Rtty => f.set_rtty_armed(false),
                DecoderScreen::Psk => f.set_psk_armed(false),
                DecoderScreen::Jtty => f.set_jtty_armed(false),
                DecoderScreen::Sstv => f.set_sstv_armed(false),
                DecoderScreen::Aprs => f.set_aprs_arm(AprsArm::Off),
            }
            f.leave_decoder_screen(s);
            assert!(
                !auto_arm(&mut f, s),
                "{s:?}: the operator's stop still holds"
            );
        }
    }

    /// A screen still on show in its own window (the SSTV picture viewer) is still the
    /// section: its decoder keeps running, and the caller is told to ask again.
    #[test]
    fn a_screen_still_shown_in_its_own_window_keeps_its_decoder() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.sstv_auto_arm());
        e.set_sstv_viewer_open(true);
        assert!(
            e.leave_decoder_screen(DecoderScreen::Sstv),
            "kept: ask again"
        );
        assert!(e.sstv_armed());
        e.set_sstv_viewer_open(false);
        assert!(
            !e.leave_decoder_screen(DecoderScreen::Sstv),
            "the window closed"
        );
        assert!(!e.sstv_armed());
    }

    /// A computer driving the station is attached: one engine, two screens (the shack window's and
    /// that computer's), each sending its own leaves. A leave cannot tell whether the other screen
    /// still shows the decoder, so while one is attached a leave stops nothing.
    fn with_a_computer_attached() -> (Engine, TransmitAuthority, Instant) {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let presence = TransmitAuthority::default();
        let t0 = Instant::now();
        let permit = presence.permit(t0 + Duration::from_secs(5)).unwrap();
        assert!(e.hold_remote_presence(permit, t0));
        (e, presence, t0)
    }

    #[test]
    fn while_a_computer_drives_the_station_no_leave_stops_a_decoder() {
        for s in SCREENS {
            let (mut e, _presence, _t0) = with_a_computer_attached();
            // The shack window and the computer both opened the screen…
            assert!(auto_arm(&mut e, s), "{s:?}: the screen arms");
            assert!(
                !auto_arm(&mut e, s),
                "{s:?}: the second screen finds it running"
            );
            // …and each leaves it: neither may stop it under the other.
            assert!(e.leave_decoder_screen(s), "{s:?}: kept, ask again");
            assert!(
                e.leave_decoder_screen(s),
                "{s:?}: the other screen's leave too"
            );
            assert!(
                armed(&e, s),
                "{s:?}: still decoding while the computer is attached"
            );
        }
    }

    /// The computer goes: what was left while it was attached stops at once — the shack does not
    /// run every decoder it ever opened for the rest of the session.
    #[test]
    fn when_the_computer_goes_what_was_left_stops() {
        let (mut e, _presence, t0) = with_a_computer_attached();
        for s in SCREENS {
            assert!(auto_arm(&mut e, s));
            assert!(e.leave_decoder_screen(s));
        }
        e.poll_remote_stream(t0 + Duration::from_secs(1));
        assert!(
            SCREENS.iter().all(|s| armed(&e, *s)),
            "still attached: all running"
        );
        let lapsed = t0 + Duration::from_secs(6);
        e.poll_remote_stream(lapsed); // presence lapsed: the grace starts
        assert!(
            SCREENS.iter().all(|s| armed(&e, *s)),
            "inside the grace: all running"
        );
        e.poll_remote_stream(lapsed + DETACH_GRACE);
        for s in SCREENS {
            assert!(!armed(&e, s), "{s:?}: stopped once the computer was gone");
            assert!(
                auto_arm(&mut e, s),
                "{s:?}: no decline: the screen arms it again"
            );
        }
        // And the next leave, with nobody attached, stops at once.
        assert!(!e.leave_decoder_screen(DecoderScreen::Psk));
        assert!(!e.psk_armed());
    }

    /// A screen opened again after its leave was held is in front: the computer going does not
    /// stop it. And a held leave that something still needs waits for that to end.
    #[test]
    fn a_held_leave_is_cancelled_by_coming_back_and_waits_for_what_still_needs_it() {
        let (mut e, _presence, t0) = with_a_computer_attached();
        assert!(e.rtty_auto_arm() && e.sstv_auto_arm());
        e.leave_decoder_screen(DecoderScreen::Rtty);
        e.leave_decoder_screen(DecoderScreen::Sstv);
        assert!(!e.rtty_auto_arm(), "back on RTTY (already running)");
        e.set_sstv_viewer_open(true);
        let lapsed = t0 + Duration::from_secs(6);
        e.poll_remote_stream(lapsed);
        e.poll_remote_stream(lapsed + DETACH_GRACE);
        assert!(e.rtty_armed(), "RTTY is in front again: not stopped");
        assert!(e.sstv_armed(), "the viewer still shows SSTV");
        e.set_sstv_viewer_open(false);
        e.poll_remote_stream(lapsed + DETACH_GRACE + Duration::from_secs(1));
        assert!(
            !e.sstv_armed(),
            "the viewer closed: the held leave is carried out"
        );
    }

    /// A computer can hold the station's lease, and drive it, with no presence at all: on the
    /// internet with datagrams blocked ("watch, tune and log only"), or on a LAN where its Stop road
    /// is not heard. It is just as attached: its screens send leaves too, and its picture viewer
    /// watches the shack's SSTV receiver. The lease counts, read off the radio loop's authority poll.
    fn with_a_lease_and_no_presence() -> (Engine, Arc<AtomicBool>, Instant) {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let lease = Arc::new(AtomicBool::new(true));
        let held = lease.clone();
        e.set_remote_tx_hold_source(Arc::new(move |_| {
            Some(RemoteTxHold {
                lease_held: held.load(Ordering::SeqCst),
                ..Default::default()
            })
        }));
        let t0 = Instant::now();
        e.poll_remote_transmit(t0);
        assert!(!e.remote_presence_live(t0), "premise: no presence");
        assert!(
            !e.remote_transmit_held(),
            "a held lease holds nothing from transmit"
        );
        (e, lease, t0)
    }

    #[test]
    fn a_lease_without_presence_is_attached_too() {
        for s in SCREENS {
            let (mut e, lease, t0) = with_a_lease_and_no_presence();
            assert!(auto_arm(&mut e, s));
            assert!(
                e.leave_decoder_screen(s),
                "{s:?}: kept while the lease is held"
            );
            assert!(e.leave_decoder_screen(s), "{s:?}: either window's leave");
            e.poll_remote_transmit(t0 + Duration::from_secs(30));
            assert!(
                armed(&e, s),
                "{s:?}: still decoding while the lease is held"
            );
            // The computer goes: the lease ends, and after the grace the held leave is carried out.
            lease.store(false, Ordering::SeqCst);
            let gone = t0 + Duration::from_secs(31);
            e.poll_remote_transmit(gone);
            e.poll_remote_transmit(gone + DETACH_GRACE - Duration::from_millis(1));
            assert!(armed(&e, s), "{s:?}: inside the grace nothing is stopped");
            e.poll_remote_transmit(gone + DETACH_GRACE);
            assert!(!armed(&e, s), "{s:?}: the grace is over: stopped");
        }
    }

    /// A gap of a few seconds (a network blip, a client window whose timers were throttled) is not
    /// the computer going: it comes back inside the grace, and nothing it was watching stopped.
    #[test]
    fn a_short_gap_carries_out_nothing() {
        let (mut e, lease, t0) = with_a_lease_and_no_presence();
        assert!(e.sstv_auto_arm());
        assert!(e.leave_decoder_screen(DecoderScreen::Sstv));
        lease.store(false, Ordering::SeqCst);
        e.poll_remote_transmit(t0 + Duration::from_secs(1));
        e.poll_remote_transmit(t0 + Duration::from_secs(5));
        lease.store(true, Ordering::SeqCst);
        e.poll_remote_transmit(t0 + Duration::from_secs(6));
        // Gone again later: the grace starts over from this gap, not the first.
        lease.store(false, Ordering::SeqCst);
        e.poll_remote_transmit(t0 + Duration::from_secs(7));
        e.poll_remote_transmit(
            t0 + Duration::from_secs(7) + DETACH_GRACE - Duration::from_millis(1),
        );
        assert!(e.sstv_armed(), "the blip carried nothing out");
        e.poll_remote_transmit(t0 + Duration::from_secs(7) + DETACH_GRACE);
        assert!(!e.sstv_armed());
    }

    /// The RTTY auto-sequencer decides its next over from what the decoder hears. While it is
    /// on, the decoder it reads must not stop behind it — that would change what it sends.
    #[test]
    fn the_rtty_auto_sequencer_keeps_its_decoder() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.rtty_auto_arm());
        e.rtty_seq = Some(tempo_core::rtty::RttySeq::new(
            "W9XYZ",
            tempo_core::contest::casual(),
            &[("RST", "599")],
        ));
        assert!(
            e.leave_decoder_screen(DecoderScreen::Rtty),
            "kept: ask again"
        );
        assert!(e.rtty_armed(), "the sequencer still hears the band");
        e.rtty_seq = None;
        assert!(!e.leave_decoder_screen(DecoderScreen::Rtty));
        assert!(!e.rtty_armed(), "Auto off: the next leave stops it");
    }

    /// ⭐ RTTY AUTO NEVER RUNS OVER A STOPPED DECODER. With a computer driving the station there
    /// are two windows and one engine: the shack window leaving RTTY stops the decoder under the
    /// client's RTTY screen, whose entry arm has already happened. Switching Auto on there must
    /// start the decoder again, or the sequencer runs deaf and every over follows its reply
    /// timer. Receive only: this arms the decoder and changes nothing the sequencer sends.
    #[test]
    fn switching_rtty_auto_on_starts_a_decoder_a_leave_stopped() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.rtty_auto_arm());
        e.leave_decoder_screen(DecoderScreen::Rtty); // the other window left RTTY
        assert!(!e.rtty_armed(), "premise: the leave stopped it");
        e.set_rtty_auto(true).unwrap();
        assert!(e.rtty_armed(), "Auto on starts the decoder it reads");

        // …but never one the operator stopped: their Stop still holds.
        let mut f = Engine::new("W9XYZ", "EN61", 0);
        f.set_rtty_armed(true);
        f.set_rtty_armed(false);
        f.set_rtty_auto(true).unwrap();
        assert!(!f.rtty_armed(), "the operator's stop is theirs");
    }

    /// Auto is the operator's commitment, so the decoder it runs on becomes theirs: after Auto
    /// goes off again a leave does not stop it (they stop it with its own button).
    #[test]
    fn switching_rtty_auto_on_makes_the_decoder_the_operators() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.rtty_auto_arm());
        e.set_rtty_auto(true).unwrap();
        assert!(
            !e.leave_decoder_screen(DecoderScreen::Rtty),
            "Auto on: the decoder is no longer the screen's to stop"
        );
        assert!(e.rtty_armed(), "Auto on, screen left: still decoding");
        e.set_rtty_auto(false).unwrap();
        assert!(
            !e.leave_decoder_screen(DecoderScreen::Rtty),
            "nothing to ask"
        );
        assert!(e.rtty_armed(), "Auto made it the operator's");
    }

    /// The RX iGate contributes what the RF decoder hears, from any screen: with it on, the
    /// APRS decoder keeps running.
    #[test]
    fn the_aprs_igate_keeps_its_decoder() {
        let mut e = Engine::with_settings(Settings {
            aprs_is_uplink: true,
            ..Default::default()
        });
        assert!(e.aprs_auto_arm());
        assert!(
            e.leave_decoder_screen(DecoderScreen::Aprs),
            "kept: ask again"
        );
        assert!(e.aprs_armed(), "the iGate still hears the channel");
        e.settings.aprs_is_uplink = false;
        assert!(!e.leave_decoder_screen(DecoderScreen::Aprs));
        assert!(!e.aprs_armed());
        assert_eq!(e.aprs_arm_source(), AprsArm::Off);
    }

    /// A stopped decoder draws nothing and holds no audio: the tap is emptied and the status
    /// the screen reads says it is off, as the ISS unwind leaves SSTV.
    #[test]
    fn a_stopped_decoder_holds_no_audio_and_reads_off() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        for s in SCREENS {
            assert!(auto_arm(&mut e, s));
        }
        e.feed_rx_audio(&[0.1; 480]);
        for s in SCREENS {
            e.leave_decoder_screen(s);
        }
        assert!(e.take_rtty_audio().is_empty());
        assert!(e.take_psk_audio().is_empty());
        assert!(e.take_aprs_audio().is_empty());
        assert!(e.take_sstv_audio().is_empty());
        assert!(e.take_jtty_audio().is_empty());
        assert!(!e.aprs_health().arm.is_armed(), "APRS health reads off");
        assert!(!e.sstv_health().armed, "SSTV health reads off");
        // No audio arrives while stopped.
        e.feed_rx_audio(&[0.1; 480]);
        assert!(e.take_rtty_audio().is_empty() && e.take_sstv_audio().is_empty());
    }
}
