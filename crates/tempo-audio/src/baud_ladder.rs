//! Test-CAT baud-ladder diagnosis for Icom CI-V rigs — the "zero bytes" root-causer.
//!
//! FIELD FAILURE this exists for (IC-7610, 2026-07): the rig's USB CI-V port ships
//! factory-set to "Link to [REMOTE]", which caps it at the REMOTE-jack rate (≤ 19200,
//! usually Auto), while Nexus is configured for the 115200 the native scope needs. A
//! CI-V rig that can't clock our request never transmits ANYTHING — so the operator sees
//! "rig reply incomplete … (got \"\")" with zero bytes, in Hamlib mode and native mode
//! alike, and nothing says which side to fix. The same zero-byte signature also comes
//! from picking the WRONG of the rig's two COM ports (the dual-UART "Standard"/"Serial
//! Port B" side never speaks CI-V).
//!
//! So: when Test CAT fails on a serial Icom, walk the SAME port through the common CI-V
//! rates (configured rate first) with a direct, read-only CI-V frequency query — no
//! Hamlib in the loop — and tell the operator exactly which side to change, or that the
//! port itself is the wrong one. Read-only by construction: the only frame ever written
//! is `read_freq` (cmd `0x03`); nothing here can key or retune a radio.
//!
//! ⚠️ The real probe ([`run`]) must only be called while the radio loop has RELEASED the
//! CAT serial port (`Engine::hold_cat_port` → loop drops its daemon → ack): serial ports
//! are exclusive-open, and our own live daemon holds the port even when the rig is mute.
//! It runs in the `test_cat` command context, never inside the radio loop tick.
//!
//! Pure pieces (ladder order, reply classification, message composition) are unit-tested
//! here in the style of [`crate::control_line`]; only [`run`]/`probe_port_baud` touch a
//! real port, behind the `serial` feature.

use crate::civ::frame::{bcd_to_freq, FrameSplitter};

/// The common Icom CI-V rates tried after the configured one, most-likely first:
/// 19200 is the "Link to [REMOTE]" ceiling (and the Auto handshake rate), then the
/// older fixed rates, then the remaining "CI-V USB Baud Rate" menu picks (38400/57600 —
/// every selectable value must be here, or a rig set to one of them walks the whole
/// ladder silent and gets misdiagnosed as the wrong COM port), then 115200 for an
/// operator who configured something slower than the rig's USB default.
pub const LADDER_BAUDS: &[u32] = &[19200, 9600, 4800, 38400, 57600, 115200];

/// The CI-V bus address to probe `rig_model` at — `Some` only for the Icoms whose
/// default address Nexus knows (the native-CI-V-capable set; the IC-7610 is 0x98).
pub fn icom_civ_addr(rig_model: u32) -> Option<u8> {
    crate::rigmodels::icom_scope_model(rig_model).map(|m| m.default_civ_addr())
}

/// Does this Icom's built-in USB enumerate TWO virtual COM ports? Only the IC-7610 and
/// IC-9700 carry the dual-UART CP2105 ("Enhanced"/"Standard"); the IC-7300/705/905 show a
/// single port, so "try the other COM port" advice would send their owners hunting for a
/// port that does not exist. Must stay in step with the UI's dual-port hint
/// (SettingsPanel `[3078, 3081]`).
pub fn dual_com_ports(rig_model: u32) -> bool {
    use crate::civ::commands::IcomModel::{Ic7610, Ic9700};
    matches!(
        crate::rigmodels::icom_scope_model(rig_model),
        Some(Ic7610 | Ic9700)
    )
}

/// Should a failed Test CAT run the ladder at all? Only for a KNOWN Icom on a real
/// serial port — the ladder speaks raw CI-V, which means nothing to a network rig,
/// a non-Icom, or an empty port field — and only when the failed probe exercised the
/// CAT channel itself. Mirrors the radio loop's `probed_cat` attribution predicate
/// (service.rs `reprobe`): "cat"/"vox" probe CAT; "rts"/"dtr" probe CAT only when
/// keying shares the CAT port (`ptt_serial_port` empty or equal to `serial_port`,
/// case-insensitively, like `Transport::ptt_port`). A dedicated-PTT-port failure is a
/// PTT problem — probing the (healthy) CAT port would find the rig answering at the
/// configured rate and REPLACE the real "Could not open serial port" error with a
/// verdict blaming a backend that never failed. Returns the CI-V address to probe at.
pub fn ladder_applies(
    is_network: bool,
    rig_model: u32,
    serial_port: &str,
    ptt_method: &str,
    ptt_serial_port: &str,
) -> Option<u8> {
    if is_network || serial_port.trim().is_empty() {
        return None;
    }
    let probed_cat = match ptt_method {
        "cat" | "vox" => true,
        "rts" | "dtr" => {
            let ptt = ptt_serial_port.trim();
            ptt.is_empty() || ptt.eq_ignore_ascii_case(serial_port.trim())
        }
        _ => false,
    };
    if !probed_cat {
        return None;
    }
    icom_civ_addr(rig_model)
}

/// The rates to try, in order: the configured rate first (re-checked directly, without
/// Hamlib in the loop), then [`LADDER_BAUDS`] minus the configured one.
pub fn ladder_bauds(configured: u32) -> Vec<u32> {
    let mut out = vec![configured];
    out.extend(LADDER_BAUDS.iter().copied().filter(|&b| b != configured));
    out
}

/// What one (port, baud) probe observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaudProbe {
    /// A well-formed CI-V frame came back from the rig — proof of life at this rate.
    /// `freq_hz` when the frame carried a readable frequency (cmd `03` reply or a
    /// `00` transceive broadcast).
    Reply { freq_hz: Option<u64> },
    /// Bytes arrived but no valid rig frame (line noise, a non-CI-V device, or only
    /// our own echo).
    Noise,
    /// The port opened cleanly and returned nothing at all.
    Silence,
    /// The port could not be opened (OS error text verbatim).
    OpenFailed(String),
}

/// Classify the raw bytes one probe read back. Echoes of our own query (`from ==
/// CONTROLLER`) are NOT proof the rig answered — the splitter drops them, so an
/// echo-only read classifies as [`BaudProbe::Noise`].
pub fn classify_probe_bytes(raw: &[u8]) -> BaudProbe {
    if raw.is_empty() {
        return BaudProbe::Silence;
    }
    let frames = FrameSplitter::new().push(raw);
    if frames.is_empty() {
        return BaudProbe::Noise;
    }
    // A frequency, when one of the frames carries it: a `03` read-freq reply or a `00`
    // transceive broadcast — both hold 5-byte little-endian BCD.
    let freq_hz = frames
        .iter()
        .find(|f| matches!(f.cmd, 0x00 | 0x03) && f.data.len() >= 5)
        .map(|f| bcd_to_freq(&f.data[..5]));
    BaudProbe::Reply { freq_hz }
}

/// Everything the ladder observed, ready for [`compose_ladder_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderReport {
    pub port: String,
    pub configured_baud: u32,
    /// `(baud, outcome)` in probe order; stops early after the first [`BaudProbe::Reply`].
    pub outcomes: Vec<(u32, BaudProbe)>,
}

/// Walk [`ladder_bauds`] with `probe`, stopping at the first rate that gets a
/// [`BaudProbe::Reply`] (the diagnosis is complete at that point). Pure orchestration —
/// `probe` is the only thing that touches hardware, exactly like
/// [`crate::control_line::open_first_working_baud`].
pub fn run_ladder(
    port: &str,
    configured_baud: u32,
    mut probe: impl FnMut(u32) -> BaudProbe,
) -> LadderReport {
    let mut outcomes = Vec::new();
    for baud in ladder_bauds(configured_baud) {
        let outcome = probe(baud);
        let done = matches!(outcome, BaudProbe::Reply { .. });
        outcomes.push((baud, outcome));
        if done {
            break; // the diagnosis is complete — a rate answered
        }
    }
    LadderReport {
        port: port.to_string(),
        configured_baud,
        outcomes,
    }
}

/// Turn a [`LadderReport`] into the operator-facing Test-CAT verdict: what answered
/// (if anything), and exactly what to change on which side. `native_selected` = the
/// radio is opted into the native CI-V backend (adds the diagnostic-log pointer);
/// `dual_ports` = the model enumerates two COM ports ([`dual_com_ports`]) — gates the
/// "try the other COM port" advice out of single-port rigs' verdicts.
pub fn compose_ladder_message(
    r: &LadderReport,
    model_name: &str,
    civ_addr: u8,
    native_selected: bool,
    dual_ports: bool,
) -> String {
    let port = &r.port;
    let configured = r.configured_baud;
    let mhz = |freq_hz: Option<u64>| {
        freq_hz
            .filter(|&hz| hz > 0)
            .map(|hz| format!(" (reads {:.3} MHz)", hz as f64 / 1e6))
            .unwrap_or_default()
    };
    // The rig answered at the CONFIGURED rate when probed directly → the serial side is
    // fine and the CAT backend between us and the port is what fell over.
    if let Some((_, BaudProbe::Reply { freq_hz })) = r
        .outcomes
        .first()
        .filter(|(b, o)| *b == configured && matches!(o, BaudProbe::Reply { .. }))
    {
        let backend = if native_selected {
            "the native CI-V daemon"
        } else {
            "the bundled rigctld (Hamlib)"
        };
        let diag = if native_selected {
            " If it keeps failing, turn on the CI-V diagnostic log (Settings » Radio) and send \
             the capture with a bug report."
        } else {
            ""
        };
        return format!(
            "The rig answers CI-V directly on {port} @ {configured} baud{f} — port, cable and \
             baud are all fine, so {backend} is what failed. Save the settings again to relaunch \
             it.{diag}",
            f = mhz(*freq_hz)
        );
    }
    // Another rate answered → say exactly which side to change, both ways.
    if let Some((baud, freq_hz)) = r.outcomes.iter().find_map(|(b, o)| match o {
        BaudProbe::Reply { freq_hz } => Some((*b, *freq_hz)),
        _ => None,
    }) {
        let why = if configured > 19_200 {
            format!(
                " (At the factory default \"Link to [REMOTE]\" the USB CI-V port follows the \
                 slower [REMOTE]-jack rate, which tops out at 19200 — that is why {configured} \
                 got silence.)"
            )
        } else {
            String::new()
        };
        return format!(
            "Found it: the rig answers CI-V on {port} at {baud} baud{f}, not the configured \
             {configured}. Fix either side — set Baud to {baud} here in Settings, or set the rig \
             to {configured}: MENU » SET » Connectors » CI-V » \"CI-V USB Baud Rate\" = \
             {configured} and \"CI-V USB Port\" = \"Unlink from [REMOTE]\".{why}",
            f = mhz(freq_hz)
        );
    }
    // No rate answered. Say why that usually is, in the order it actually happens.
    let tried = r
        .outcomes
        .iter()
        .map(|(b, _)| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if r.outcomes
        .iter()
        .all(|(_, o)| matches!(o, BaudProbe::OpenFailed(_)))
    {
        let os_err = r
            .outcomes
            .iter()
            .find_map(|(_, o)| match o {
                BaudProbe::OpenFailed(e) => Some(e.as_str()),
                _ => None,
            })
            .unwrap_or("unknown error");
        return format!(
            "Test CAT could not open {port} at any rate (tried {tried}) — the system said: \
             {os_err}. Usually another program is holding the port — close other CAT/logging \
             software (WSJT-X, flrig, RS-BA1) and test again."
        );
    }
    let noise = if r.outcomes.iter().any(|(_, o)| matches!(o, BaudProbe::Noise)) {
        " The port did carry bytes at one rate, but not valid CI-V — that usually means a \
         different device is on this COM port."
    } else {
        ""
    };
    let port_identity = if dual_ports {
        format!(
            "This usually means {port} is not the rig's CI-V port: this Icom's USB shows up as \
             TWO COM ports and only one speaks CI-V. In Windows Device Manager (Ports), the \
             CI-V one is the CP210x port marked \"Enhanced\" (Icom's driver labels it \"Serial \
             Port A (CI-V)\"); the \"Standard\" / \"Serial Port B\" one never answers — try the \
             other COM port."
        )
    } else {
        format!(
            "This usually means {port} is not the rig: {model_name} shows a single COM port — \
             unplug the rig's USB cable and confirm {port} is the one that disappears from \
             Device Manager (Ports), then reconnect."
        )
    };
    format!(
        "{model_name} on {port} never answered CI-V at any rate (tried {tried}).{noise} \
         {port_identity} Also check: the radio is on; the rig menu CI-V Address is at its \
         default ({civ_addr:02X}h); and if no COM ports appear at all, install Icom's USB \
         driver."
    )
}

/// One real (port, baud) probe: open, send a single read-only CI-V `read_freq`, gather
/// whatever comes back for ~600 ms, classify.
#[cfg(feature = "serial")]
fn probe_port_baud(port: &str, baud: u32, civ_addr: u8) -> BaudProbe {
    use std::io::Read;
    use std::time::{Duration, Instant};
    let mut sp = match serialport::new(port, baud)
        .timeout(Duration::from_millis(50))
        .open()
    {
        Ok(sp) => sp,
        Err(e) => return BaudProbe::OpenFailed(e.to_string()),
    };
    let query = crate::civ::commands::read_freq(civ_addr).to_bytes();
    if let Err(e) = std::io::Write::write_all(&mut sp, &query).and_then(|()| sp.flush()) {
        return BaudProbe::OpenFailed(e.to_string());
    }
    let mut raw = Vec::new();
    let mut buf = [0u8; 256];
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        match sp.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                // A frame terminator is in hand — classify now rather than sitting
                // out the rest of the window.
                if raw.contains(&crate::civ::frame::END)
                    && matches!(classify_probe_bytes(&raw), BaudProbe::Reply { .. })
                {
                    break;
                }
            }
            // Timeout = no bytes this tick; anything else = the port died mid-read.
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                if raw.is_empty() {
                    return BaudProbe::OpenFailed(e.to_string());
                }
                break;
            }
        }
    }
    classify_probe_bytes(&raw)
}

/// The real ladder: probe `port` at [`ladder_bauds`] until a rate answers. Blocking for
/// up to ~3 s — call from the `test_cat` command (off the radio loop), with the loop's
/// CAT port hold acknowledged.
#[cfg(feature = "serial")]
pub fn run(port: &str, configured_baud: u32, civ_addr: u8) -> LadderReport {
    run_ladder(port, configured_baud, |baud| {
        probe_port_baud(port, baud, civ_addr)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ::frame::Frame;

    // ---- ladder order ----

    #[test]
    fn ladder_tries_the_configured_rate_first_then_the_common_civ_rates() {
        // The native-scope config (the field failure): 115200 configured → re-check it
        // directly, then the "Link to [REMOTE]" ceiling first among the alternatives.
        assert_eq!(
            ladder_bauds(115200),
            vec![115200, 19200, 9600, 4800, 38400, 57600]
        );
        // A rig-menu rate already configured → 115200 still gets tried (last).
        assert_eq!(
            ladder_bauds(19200),
            vec![19200, 9600, 4800, 38400, 57600, 115200]
        );
        // A configured rate from the middle of the set is never probed twice.
        assert_eq!(
            ladder_bauds(38400),
            vec![38400, 19200, 9600, 4800, 57600, 115200]
        );
    }

    #[test]
    fn every_civ_usb_baud_rate_menu_value_is_on_the_ladder() {
        // The rig menu the verdicts name ("CI-V USB Baud Rate") offers exactly these
        // fixed rates; any one missing here walks the ladder silent and gets the
        // wrong-COM-port verdict instead of the baud-mismatch one.
        for menu_rate in [4800u32, 9600, 19200, 38400, 57600, 115200] {
            assert!(
                LADDER_BAUDS.contains(&menu_rate),
                "{menu_rate} is a CI-V USB Baud Rate menu value but not on the ladder"
            );
        }
    }

    #[test]
    fn ladder_applies_only_to_a_known_icom_on_a_real_serial_port() {
        assert_eq!(ladder_applies(false, 3078, "COM4", "cat", ""), Some(0x98)); // IC-7610
        assert_eq!(ladder_applies(false, 3073, "COM4", "cat", ""), Some(0x94)); // IC-7300
        assert_eq!(ladder_applies(true, 3078, "COM4", "cat", ""), None); // network rig
        assert_eq!(ladder_applies(false, 1042, "COM4", "cat", ""), None); // Yaesu — no CI-V
        assert_eq!(ladder_applies(false, 0, "COM4", "cat", ""), None); // no model
        assert_eq!(ladder_applies(false, 3078, "  ", "cat", ""), None); // no port
    }

    #[test]
    fn the_ladder_only_runs_when_the_failed_probe_was_a_cat_probe() {
        // "cat" and "vox" (with an Icom model) probe the CAT channel → ladder applies.
        assert_eq!(ladder_applies(false, 3078, "COM4", "cat", ""), Some(0x98));
        assert_eq!(ladder_applies(false, 3078, "COM4", "vox", ""), Some(0x98));
        // Dedicated-port RTS/DTR keying: reprobe tested only the PTT line — its failure
        // says nothing about the CAT port, and a ladder run would bury the real
        // "Could not open serial port COM5" error under a bogus backend verdict.
        assert_eq!(ladder_applies(false, 3078, "COM4", "rts", "COM5"), None);
        assert_eq!(ladder_applies(false, 3078, "COM4", "dtr", "COM5"), None);
        // Shared-port keying (dedicated port empty, or equal ignoring case): rigctld owns
        // the CAT port and the reprobe DID probe it → ladder applies.
        assert_eq!(ladder_applies(false, 3078, "COM4", "rts", ""), Some(0x98));
        assert_eq!(ladder_applies(false, 3078, "COM4", "dtr", "com4"), Some(0x98));
        assert_eq!(ladder_applies(false, 3078, "COM4", "rts", " COM4 "), Some(0x98));
    }

    #[test]
    fn only_the_dual_uart_icoms_get_two_port_advice() {
        // Must stay in step with the UI's [3078, 3081] dual-port hint (SettingsPanel).
        assert!(dual_com_ports(3078)); // IC-7610 — CP2105 dual UART
        assert!(dual_com_ports(3081)); // IC-9700 — CP2105 dual UART
        assert!(!dual_com_ports(3073)); // IC-7300 — single port
        assert!(!dual_com_ports(3085)); // IC-705 — single port
        assert!(!dual_com_ports(3090)); // IC-905 — single port
        assert!(!dual_com_ports(1042)); // non-Icom
        assert!(!dual_com_ports(0));
    }

    // ---- reply classification ----

    #[test]
    fn a_freq_reply_frame_classifies_as_reply_with_the_frequency() {
        // IC-7610 answering read_freq: FE FE E0 98 03 <BCD 14.074.000> FD
        let mut f = Frame::command(0x98, 0x03, &crate::civ::frame::freq_to_bcd(14_074_000));
        (f.to, f.from) = (crate::civ::frame::CONTROLLER, 0x98);
        assert_eq!(
            classify_probe_bytes(&f.to_bytes()),
            BaudProbe::Reply {
                freq_hz: Some(14_074_000)
            }
        );
    }

    #[test]
    fn a_transceive_broadcast_also_proves_life_and_carries_the_freq() {
        // CI-V Transceive ON: the rig broadcasts `00` frames to address 00 as the dial
        // moves — proof of life at this rate even if our own query got no direct answer.
        let mut f = Frame::command(0x00, 0x00, &crate::civ::frame::freq_to_bcd(7_074_000));
        (f.to, f.from) = (0x00, 0x98);
        assert_eq!(
            classify_probe_bytes(&f.to_bytes()),
            BaudProbe::Reply {
                freq_hz: Some(7_074_000)
            }
        );
    }

    #[test]
    fn our_own_echo_is_not_a_rig_reply() {
        // USB Echo Back ON echoes the query verbatim (from == CONTROLLER). Bytes arrived
        // but the rig said nothing — that must NOT read as a working link.
        let echo = crate::civ::commands::read_freq(0x98).to_bytes();
        assert_eq!(classify_probe_bytes(&echo), BaudProbe::Noise);
    }

    #[test]
    fn garbage_is_noise_and_nothing_is_silence() {
        assert_eq!(classify_probe_bytes(&[0x55, 0xAA, 0x00]), BaudProbe::Noise);
        assert_eq!(classify_probe_bytes(&[]), BaudProbe::Silence);
    }

    #[test]
    fn an_ack_frame_with_no_freq_still_proves_life() {
        let mut f = Frame::command(0x98, crate::civ::frame::OK, &[]);
        (f.to, f.from) = (crate::civ::frame::CONTROLLER, 0x98);
        assert_eq!(
            classify_probe_bytes(&f.to_bytes()),
            BaudProbe::Reply { freq_hz: None }
        );
    }

    // ---- ladder orchestration ----

    #[test]
    fn the_ladder_stops_at_the_first_rate_that_replies() {
        let mut tried = Vec::new();
        let r = run_ladder("COM4", 115200, |baud| {
            tried.push(baud);
            if baud == 19200 {
                BaudProbe::Reply {
                    freq_hz: Some(14_074_000),
                }
            } else {
                BaudProbe::Silence
            }
        });
        assert_eq!(tried, vec![115200, 19200], "9600/4800 must not be probed");
        assert_eq!(r.outcomes.len(), 2);
        assert_eq!(r.outcomes[0], (115200, BaudProbe::Silence));
        assert_eq!(
            r.outcomes[1],
            (
                19200,
                BaudProbe::Reply {
                    freq_hz: Some(14_074_000)
                }
            )
        );
    }

    #[test]
    fn a_totally_silent_port_walks_every_rate() {
        let r = run_ladder("COM4", 115200, |_| BaudProbe::Silence);
        assert_eq!(
            r.outcomes.iter().map(|(b, _)| *b).collect::<Vec<_>>(),
            vec![115200, 19200, 9600, 4800, 38400, 57600]
        );
    }

    // ---- message composition ----

    fn report(configured: u32, outcomes: Vec<(u32, BaudProbe)>) -> LadderReport {
        LadderReport {
            port: "COM4".into(),
            configured_baud: configured,
            outcomes,
        }
    }

    #[test]
    fn a_hit_at_another_rate_names_both_fixes_and_the_exact_rig_menu() {
        // THE field case: configured 115200, rig still linked to [REMOTE] → answers at 19200.
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::Silence),
                (
                    19200,
                    BaudProbe::Reply {
                        freq_hz: Some(14_074_000),
                    },
                ),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, true, true);
        assert!(m.contains("COM4"), "{m}");
        assert!(m.contains("19200"), "must name the answering rate: {m}");
        assert!(m.contains("14.074"), "must show the read frequency: {m}");
        // Option 1: fix the app side.
        assert!(m.contains("Baud"), "{m}");
        // Option 2: fix the rig side, with the exact menu path.
        assert!(m.contains("MENU"), "{m}");
        assert!(m.contains("CI-V USB Baud Rate"), "{m}");
        assert!(m.contains("Unlink from [REMOTE]"), "{m}");
        // And WHY 115200 was silent (the linked-port ceiling) — only relevant > 19200.
        assert!(m.contains("Link to [REMOTE]"), "{m}");
    }

    #[test]
    fn a_hit_with_a_slow_configured_rate_skips_the_remote_link_explanation() {
        let r = report(
            9600,
            vec![
                (9600, BaudProbe::Silence),
                (19200, BaudProbe::Reply { freq_hz: None }),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true);
        assert!(m.contains("19200"), "{m}");
        assert!(
            !m.contains("Link to [REMOTE]"),
            "the linked-port ceiling can't explain a silent 9600: {m}"
        );
    }

    #[test]
    fn total_silence_gives_the_two_port_identity_walkthrough() {
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::Silence),
                (19200, BaudProbe::Silence),
                (9600, BaudProbe::Silence),
                (4800, BaudProbe::Silence),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true);
        assert!(m.contains("Icom IC-7610"), "{m}");
        assert!(m.contains("115200") && m.contains("4800"), "list rates: {m}");
        assert!(m.contains("TWO COM ports"), "{m}");
        // How to tell them apart on Windows (Enhanced = CI-V side, Standard = never).
        assert!(m.contains("Enhanced"), "{m}");
        assert!(m.contains("Standard"), "{m}");
        // The remaining silent killers: power, a changed CI-V address, a missing driver.
        assert!(m.contains("CI-V Address"), "{m}");
        assert!(m.contains("98h"), "must name the expected address: {m}");
        assert!(m.contains("driver"), "{m}");
    }

    #[test]
    fn total_silence_on_a_single_port_icom_never_sends_the_operator_port_hunting() {
        // An IC-7300 (single CP2102 UART) with a dead cable/driver: there IS no second
        // COM port, so the dual-port walkthrough would be a wild-goose chase. The verdict
        // must instead verify the port's identity (unplug test) and keep the real silent
        // killers: power, CI-V address, driver.
        let r = LadderReport {
            port: "COM4".into(),
            configured_baud: 115200,
            outcomes: vec![
                (115200, BaudProbe::Silence),
                (19200, BaudProbe::Silence),
                (9600, BaudProbe::Silence),
                (4800, BaudProbe::Silence),
                (38400, BaudProbe::Silence),
                (57600, BaudProbe::Silence),
            ],
        };
        let m = compose_ladder_message(&r, "Icom IC-7300", 0x94, false, false);
        assert!(!m.contains("TWO COM ports"), "{m}");
        assert!(!m.contains("Enhanced"), "{m}");
        assert!(!m.contains("other COM port"), "{m}");
        assert!(m.contains("single COM port"), "{m}");
        assert!(m.contains("unplug"), "identity check via the unplug test: {m}");
        assert!(m.contains("CI-V Address"), "{m}");
        assert!(m.contains("94h"), "must name the 7300's address: {m}");
        assert!(m.contains("driver"), "{m}");
    }

    #[test]
    fn a_direct_answer_at_the_configured_rate_points_at_the_backend_not_the_rig() {
        let r = report(
            115200,
            vec![(
                115200,
                BaudProbe::Reply {
                    freq_hz: Some(14_074_000),
                },
            )],
        );
        // Native backend selected → name it, and point at the CI-V diagnostic log.
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, true, true);
        assert!(m.contains("answers CI-V directly"), "{m}");
        assert!(m.contains("native CI-V"), "{m}");
        assert!(m.contains("CI-V diagnostic log"), "{m}");
        // Hamlib backend → name rigctld, and no native-only log pointer.
        let m2 = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true);
        assert!(m2.contains("rigctld"), "{m2}");
        assert!(!m2.contains("CI-V diagnostic log"), "{m2}");
    }

    #[test]
    fn an_unopenable_port_reports_the_os_error_not_a_guess() {
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::OpenFailed("Access is denied.".into())),
                (19200, BaudProbe::OpenFailed("Access is denied.".into())),
                (9600, BaudProbe::OpenFailed("Access is denied.".into())),
                (4800, BaudProbe::OpenFailed("Access is denied.".into())),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true);
        assert!(m.contains("Access is denied."), "{m}");
        assert!(m.contains("another program"), "{m}");
    }

    #[test]
    fn noise_without_a_valid_frame_says_the_port_is_talking_but_not_civ() {
        let r = report(
            115200,
            vec![
                (115200, BaudProbe::Noise),
                (19200, BaudProbe::Silence),
                (9600, BaudProbe::Silence),
                (4800, BaudProbe::Silence),
            ],
        );
        let m = compose_ladder_message(&r, "Icom IC-7610", 0x98, false, true);
        assert!(m.contains("not valid CI-V"), "{m}");
    }
}
