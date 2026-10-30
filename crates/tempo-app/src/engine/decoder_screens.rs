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
    /// The operator left `screen` for another section. Stops its decoder if, and only if, opening
    /// the screen is what started it and nothing else still needs it. `shown_elsewhere`: the
    /// screen is still on show in a window of its own, which keeps it in the section.
    ///
    /// Returns whether the decoder is STILL running on its screen's arm — kept for one of the
    /// reasons above — so the caller can ask again at the next section change. A decoder the
    /// operator armed, or one already off, returns false: there is nothing left to ask about.
    pub fn leave_decoder_screen(&mut self, screen: DecoderScreen, shown_elsewhere: bool) -> bool {
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
        let still_needed = shown_elsewhere
            || match screen {
                // The auto-sequencer reads the decode to pick its next over.
                DecoderScreen::Rtty => self.rtty_seq.is_some(),
                // The RX iGate contributes what this decoder hears.
                DecoderScreen::Aprs => self.settings.aprs_is_uplink,
                DecoderScreen::Psk | DecoderScreen::Jtty | DecoderScreen::Sstv => false,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

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
            assert!(!e.leave_decoder_screen(s, false), "{s:?}: nothing keeps it");
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
        e.leave_decoder_screen(DecoderScreen::Sstv, false);
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
            assert!(!e.leave_decoder_screen(s, false), "{s:?}: nothing to ask");
            assert!(armed(&e, s), "{s:?}: the operator's arm survives");

            let mut f = Engine::new("W9XYZ", "EN61", 0);
            assert!(auto_arm(&mut f, s));
            operator_arm(&mut f, s);
            f.leave_decoder_screen(s, false);
            assert!(
                armed(&f, s),
                "{s:?}: an explicit arm after entry is the operator's"
            );
        }
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        assert!(e.sstv_auto_arm());
        e.request_sstv_manual_rx("pd120".to_string());
        e.leave_decoder_screen(DecoderScreen::Sstv, false);
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
            e.leave_decoder_screen(s, false);
            assert!(auto_arm(&mut e, s), "{s:?}: re-entry is not refused");
            assert!(armed(&e, s), "{s:?}: running again");
            // …and it is still the screen's arm, so the next leave stops it again.
            e.leave_decoder_screen(s, false);
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
        e.leave_decoder_screen(DecoderScreen::Rtty, false);
        e.leave_decoder_screen(DecoderScreen::Psk, false);
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
            assert!(
                !e.leave_decoder_screen(s, false),
                "{s:?}: off, nothing to do"
            );
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
            f.leave_decoder_screen(s, false);
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
        assert!(
            e.leave_decoder_screen(DecoderScreen::Sstv, true),
            "kept: ask again"
        );
        assert!(e.sstv_armed());
        assert!(
            !e.leave_decoder_screen(DecoderScreen::Sstv, false),
            "the window closed"
        );
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
            e.leave_decoder_screen(DecoderScreen::Rtty, false),
            "kept: ask again"
        );
        assert!(e.rtty_armed(), "the sequencer still hears the band");
        e.rtty_seq = None;
        assert!(!e.leave_decoder_screen(DecoderScreen::Rtty, false));
        assert!(!e.rtty_armed(), "Auto off: the next leave stops it");
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
            e.leave_decoder_screen(DecoderScreen::Aprs, false),
            "kept: ask again"
        );
        assert!(e.aprs_armed(), "the iGate still hears the channel");
        e.settings.aprs_is_uplink = false;
        assert!(!e.leave_decoder_screen(DecoderScreen::Aprs, false));
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
            e.leave_decoder_screen(s, false);
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
