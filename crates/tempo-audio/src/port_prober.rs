//! CAT port auto-test — find which serial port (and baud) actually drives the rig.
//!
//! Many rigs expose several COM ports (a CAT port, a second data/PTT port, a GPS or
//! tuner port); only one answers Hamlib, and which one is unlabelled and rig-specific.
//! Rather than make the operator guess, probe each candidate port: spawn a throwaway
//! `rigctld` on a private TCP port, ask it for the dial frequency (**read-only — never
//! any TX/PTT**), and keep the first (port, baud) that returns a plausible frequency.
//!
//! The pure pieces (frequency sanity, candidate building) are unit-tested here; the
//! spawn/connect orchestration ([`probe_cat_ports`]) needs `rigctld` + real ports and is
//! validated by the Windows build + on-air.

use crate::ports::UsbPort;
use crate::usbrig::match_rig_model;

/// Baud rates tried per port, most-common-for-CAT first. A port that answers at one baud
/// ends the probe immediately (auto-select), so the rare high speeds rarely run.
///
/// **57600 is here because a rate no rig can be set to is one thing, and a rate a rig can ONLY
/// be set to is another.** The FT-847's CAT RATE (menu 37) offers exactly 4800 / 9600 / 57600;
/// an operator on the top setting was undetectable by construction, not by bad luck, and
/// Auto-test — the one tool that exists for "which port is my rig on?" — found nothing on any
/// port (field report 2026-08; his rig works in WSJT-X). Hamlib's own caps agree the rate is
/// real for that backend: `rigctl -m 1001 --dump-caps` → `Serial speed: 4800..57600`. It goes
/// LAST but one because it is rare everywhere else, and the first answer ends the sweep.
pub const PROBE_BAUDS: &[u32] = &[38400, 9600, 19200, 4800, 57600, 115200];

/// A plausible CAT dial frequency (Hz): 100 kHz .. 500 MHz covers HF through 2 m / 70 cm.
/// A port that isn't really the rig returns 0, an error, or garbage — all rejected.
pub fn is_plausible_cat_freq(hz: u64) -> bool {
    (100_000..=500_000_000).contains(&hz)
}

/// What an Auto-test sweep concluded. `Hit`/`NoAnswer` are the two honest hardware verdicts;
/// `NoRigctld` is the one that is NOT about hardware at all — the throwaway daemon itself
/// could not be spawned (`ErrorKind::NotFound`), so no port was ever probed. Before this was
/// typed, that case fell into the `NoAnswer` arm's "Check the cable and that the rig is on"
/// — blaming the operator's wiring for an uninstalled Hamlib, with the real cause swallowed
/// by a `let Ok(..) else continue` (mac QA audit, 2026-08-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// A port answered — ready to write into settings.
    Hit(ProbeHit),
    /// Every candidate was probed; none answered.
    NoAnswer,
    /// `rigctld` would not spawn (NotFound): Hamlib's tools are not installed. Carries the
    /// ready-to-show per-platform install cure ([`crate::rigctld_proc::hamlib_missing`]) so
    /// the command layer cannot re-guess it.
    NoRigctld(String),
}

/// The winning port found by [`probe_cat_ports`] — ready to write into settings.
/// (The src-tauri command maps this to its own serde DTO; tempo-audio stays serde-free.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeHit {
    pub port_name: String,
    pub baud: u32,
    pub model: u32,
    pub model_name: String,
    pub freq_hz: u64,
    /// The model was a GUESS (a common-rig seed tried because no model was configured and the USB
    /// descriptor didn't name one). The port + baud are confirmed working, but the model is not —
    /// the UI should apply port/baud and still make the operator confirm Rig Model, since a wrong
    /// same-family model (FT-991A answering the FTDX10 probe) would carry the wrong Hamlib tables.
    pub model_seeded: bool,
}

/// A port + the Hamlib model to try on it, before the baud sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub port_name: String,
    pub model: u32,
    pub model_name: String,
    /// If set, probe ONLY this baud (a seed's family default) instead of the full sweep — keeps
    /// the no-answer worst case from ballooning to minutes across every seeded model × 5 bauds.
    pub baud: Option<u32>,
    /// True = a common-rig seed (unconfirmed model); false = a native-USB match or the configured
    /// fallback (trusted model).
    pub seeded: bool,
}

/// Common CAT rigs to try when a bridge-chip port yields no model AND the operator hasn't set one
/// yet — so Auto-test can still find the port. Yaesu (and some others) report only the bridge
/// chip's name in the USB descriptor, so Detect can't identify them; that's the whole reason
/// Auto-test exists, but it used to find nothing until a model was picked (chicken-and-egg). Kept
/// to one representative per popular family so the sweep stays quick (the first that answers wins).
/// `(hamlib_model, display_name, seed_bauds)`. A seeded probe tries each listed baud (first answer
/// wins), kept short per family so the no-answer worst case stays quick.
pub const COMMON_CAT_MODELS: &[(u32, &str, &[u32])] = &[
    (1042, "Yaesu FTDX10", &[38400]),
    (1035, "Yaesu FT-991 / FT-991A", &[38400]),
    (1049, "Yaesu FT-710", &[38400]),
    (1040, "Yaesu FTDX101D", &[38400]),
    // Icom CI-V rigs each answer only at their OWN CI-V address, so the 7300 seed (0x94)
    // can't find a 7610 (0x98) or 9700 (0xA2) — each popular Icom model needs its own seed.
    // Try 115200 AND 19200: USB CI-V defaults to 115200, but the rig-menu "CI-V USB Baud Rate"
    // is operator-settable — a 7610 left at 19200 answered nothing on a 115200-only seed.
    (3073, "Icom IC-7300", &[115200, 19200]),
    (3078, "Icom IC-7610", &[115200, 19200]),
    (3081, "Icom IC-9700", &[115200, 19200]),
    (2037, "Kenwood TS-590SG", &[115200]),
    (2029, "Elecraft K3", &[38400]),
    // --- Interface-cable rigs (Digirig / RigBlaster companions), added 2026-07-26. ---
    // These are the rigs that reach this sweep MOST often and previously never matched: they
    // connect through a generic USB-serial bridge (CP2102/CH340/FTDI), so the USB product string
    // names the CHIP, not the radio — `match_rig_model` returns None and the seeded sweep is the
    // only thing that can identify them. A native-USB rig (IC-705 over its own USB, FT-710) names
    // itself and never gets here; the IC-705 seed below is for the CI-V-cable wiring of it.
    //
    // ⚠️ COST, stated rather than hidden: this roughly DOUBLES the baud-probes in the worst case
    // (12 → 23). The worst case is "nothing answers" — a wrong port, or no rig — because the first
    // answer wins. A real rig on the right port is unaffected in practice. If detect ever feels
    // slow, trim from the BOTTOM of this block, not the top.
    //
    // Bauds are each family's factory default first. All of these land on the LONG (2.5 s) CAT
    // deadline automatically — Xiegu by model, the rest by the `baud <= 19200` rule in
    // `rigmodels::is_slow_serial_link` — so a slow vintage backend is not misread as "no answer".
    (1036, "Yaesu FT-891", &[38400]),
    (1022, "Yaesu FT-857 / FT-857D", &[4800]),
    (1041, "Yaesu FT-818 / FT-818ND", &[4800]),
    (1020, "Yaesu FT-817 / FT-817ND", &[4800]),
    (3070, "Icom IC-7100", &[19200, 115200]),
    (3085, "Icom IC-705", &[115200, 19200]),
    (3088, "Xiegu G90", &[19200]),
    (3087, "Xiegu X6100", &[19200]),
    (2028, "Kenwood TS-480 (SAT/HX)", &[9600, 57600]),
];

/// Build probe candidates from enumerated USB ports. A native-USB rig (IC-705, FT-710…)
/// names its model in the USB product string → [`match_rig_model`] resolves it; a rig
/// behind a generic bridge chip (CP2102/FTDI) names only the chip → fall back to the
/// operator's currently-configured `fallback_model` (the rig is known, only the *port* is in
/// doubt), or — when no model is configured yet — seed [`COMMON_CAT_MODELS`] so Auto-test isn't
/// dead before setup. Candidates with no usable model (0) are dropped.
pub fn candidates_from(
    ports: &[UsbPort],
    fallback_model: u32,
    exclude: &[String],
) -> Vec<Candidate> {
    // `exclude` = ports already OWNED (the wizard probing for radio 2 passes radio 1's
    // serial port): probing an owned port cannot succeed — the live daemon holds it —
    // and each excluded candidate otherwise costs a full no-answer baud sweep before
    // the sweep ever reaches the port that could answer.
    //
    // Dual-UART Icoms (IC-7610/9700) enumerate as TWO ports that both resolve to the same
    // model; only the one the driver marks "A (CI-V)" / "Enhanced" ever answers. Probe that
    // one first and the known non-CI-V side last, so the right port isn't stuck waiting
    // behind a full no-answer sweep of its dead twin. Stable sort — everything without a
    // marking keeps the enumeration order.
    let mut ports: Vec<&UsbPort> = ports
        .iter()
        .filter(|p| !exclude.iter().any(|e| e.eq_ignore_ascii_case(&p.port_name)))
        .collect();
    ports.sort_by_key(|p| match crate::usbrig::civ_port_side(&p.product) {
        Some(true) => 0,
        None => 1,
        Some(false) => 2,
    });
    ports
        .iter()
        .flat_map(|p| match match_rig_model(&p.product, &p.manufacturer) {
            // Native-USB rig names its model → one exact, trusted candidate (full baud sweep).
            Some((m, name)) => vec![Candidate {
                port_name: p.port_name.clone(),
                model: m,
                model_name: name.to_string(),
                baud: None,
                seeded: false,
            }],
            // Bridge chip WITH a configured model → use it (the rig is known, only the port isn't).
            None if fallback_model > 0 => vec![Candidate {
                port_name: p.port_name.clone(),
                model: fallback_model,
                model_name: if p.product.is_empty() {
                    p.port_name.clone()
                } else {
                    p.product.clone()
                },
                baud: None,
                seeded: false,
            }],
            // Bridge chip, no model yet → try the common rigs (each at its seed bauds) so Auto-test
            // can still find the PORT; the model is a guess (flagged seeded).
            None => COMMON_CAT_MODELS
                .iter()
                .flat_map(|(m, name, bauds)| {
                    bauds.iter().map(move |b| Candidate {
                        port_name: p.port_name.clone(),
                        model: *m,
                        model_name: (*name).to_string(),
                        baud: Some(*b),
                        seeded: true,
                    })
                })
                .collect(),
        })
        .filter(|c| c.model > 0)
        .collect()
}

/// Probe every enumerated USB port for a working CAT connection and return the first
/// that reads back a plausible dial frequency (auto-select). `fallback_model` is the
/// operator's configured Hamlib model, used for ports whose USB descriptor doesn't name
/// one. `tcp_port` is a free local port for the throwaway `rigctld` (distinct from the
/// live daemon's). Read-only: it never keys the rig.
///
/// Run this when no live `rigctld` is holding the ports (the setup wizard, or a
/// not-yet-working CAT) — a daemon already bound to the real port blocks that port's
/// probe (the other ports still test fine).
///
/// Every throwaway daemon here is launched with the control lines held low
/// ([`crate::rigctld_proc::ControlLines::hold_low`]) — the sweep spawns one per (port, baud,
/// model) candidate and, before 1.0.2, every one of them left RTS asserted, so the comment
/// above was only true of the CAT traffic. It does NOT read the operator's per-line override:
/// the sweep runs before a radio is configured (the setup wizard) and probes ports that may
/// belong to no rig at all, so the safe state is the only defensible one here.
/// `exclude` — serial ports to SKIP, by name (case-insensitive): the ports other radios
/// already own. The wizard's second-radio probe passes radio 1's port so the sweep never
/// wastes a baud ladder on a port a live daemon is holding.
#[cfg(feature = "serial")]
pub fn probe_cat_ports(fallback_model: u32, tcp_port: u16, exclude: &[String]) -> ProbeOutcome {
    use crate::rig::Rig;
    use crate::rigctld_proc::{spawn_rigctld, ControlLines};
    use std::time::Duration;

    let ports = crate::ports::available_usb_ports();
    let addr = format!("127.0.0.1:{tcp_port}");
    for c in candidates_from(&ports, fallback_model, exclude) {
        // A seeded (guessed-model) candidate probes only its family baud; a trusted model sweeps.
        let bauds: &[u32] = c
            .baud
            .as_ref()
            .map(std::slice::from_ref)
            .unwrap_or(PROBE_BAUDS);
        for &baud in bauds {
            // Throwaway daemon for this (port, baud, model) — killed on drop.
            let proc = match spawn_rigctld(
                c.model,
                &c.port_name,
                baud,
                tcp_port,
                false,
                // DEFAULT (no declaration): this sweep probes ports BEFORE any of them is
                // configured, so there is no per-port declaration to read yet. Declaring RTS
                // deliberate here would drop the hardware handshake on every model it touches,
                // which is the FTDX10/FT-991 bench regression reintroduced blind.
                crate::rigctld_proc::KeyingFacts::default(),
                ControlLines::hold_low(),
            ) {
                Ok(proc) => proc,
                // The daemon BINARY is missing — deterministic, so every remaining candidate
                // would fail identically. Say so once instead of sweeping candidates that
                // cannot succeed and then blaming the cable (see [`ProbeOutcome::NoRigctld`]).
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return ProbeOutcome::NoRigctld(crate::rigctld_proc::hamlib_missing(
                        "rigctld", &e,
                    ));
                }
                // Any other spawn fault (a transient resource error) keeps the old behaviour:
                // this candidate is skipped, the sweep goes on.
                Err(_) => continue,
            };
            // Let rigctld open the port + settle, then ask for the dial frequency.
            std::thread::sleep(Duration::from_millis(700));
            let hz = Rig::rigctld(&addr)
                .read_freq()
                .ok()
                .filter(|&hz| is_plausible_cat_freq(hz));
            drop(proc); // kill rigctld + free the serial port before the next attempt
            std::thread::sleep(Duration::from_millis(200));
            if let Some(freq_hz) = hz {
                return ProbeOutcome::Hit(ProbeHit {
                    port_name: c.port_name,
                    baud,
                    model: c.model,
                    model_name: c.model_name,
                    freq_hz,
                    model_seeded: c.seeded,
                });
            }
        }
    }
    ProbeOutcome::NoAnswer
}

/// Without the `serial` feature there is no port enumeration → nothing to probe.
#[cfg(not(feature = "serial"))]
pub fn probe_cat_ports(_fallback_model: u32, _tcp_port: u16, _exclude: &[String]) -> ProbeOutcome {
    ProbeOutcome::NoAnswer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every seeded model number must be a REAL Hamlib model. A typo'd or removed number does
    /// not fail loudly — the probe just never gets an answer, and the rig reports as undetected
    /// with no clue why. The display name must match the table too, or Detect shows the operator
    /// one rig while configuring another.
    #[test]
    fn every_seeded_cat_model_exists_in_hamlib() {
        let table = crate::rigmodels::rig_models();
        for (model, name, bauds) in COMMON_CAT_MODELS {
            let found = table.iter().find(|(m, _)| m == model);
            let (_, real) = found
                .unwrap_or_else(|| panic!("seeded model {model} ({name}) is not a Hamlib model"));
            assert_eq!(
                real, name,
                "seeded model {model} is really {real:?}, not {name:?} — Detect would name the \
                 wrong radio"
            );
            assert!(
                !bauds.is_empty(),
                "{name} has no seed baud, so it can never be probed"
            );
            assert!(bauds.iter().all(|b| *b > 0), "{name} has a zero baud seed");
        }
    }

    /// THE FIELD REPORT (2026-08, FT-847). His rig works in WSJT-X and not in Nexus, and
    /// Auto-test found nothing on any port. The FT-847's CAT RATE is menu 37 and it offers
    /// **4800 / 9600 / 57600** — those three and nothing else. His radio is on 57600, which
    /// the sweep did not try, so the one tool that exists to find an unknown port could not
    /// find him at all. A rate a catalog rig's own menu offers has to be in here or that
    /// rig is undetectable by construction, not by bad luck.
    #[test]
    fn the_sweep_tries_every_rate_the_ft847s_cat_menu_offers() {
        for baud in [4800u32, 9600, 57600] {
            assert!(
                PROBE_BAUDS.contains(&baud),
                "{baud} is one of the FT-847's three CAT RATE choices (menu 37); a rig left \
                 on it can never be found by Auto-test"
            );
        }
    }

    /// The trusted-model sweep runs EVERY baud on EVERY candidate port before giving up, so
    /// its length is paid in full on a failed detect (~0.9 s per attempt). Pinned for the same
    /// reason the seeded sweep is: growth must be a decision, not a drift.
    #[test]
    fn the_trusted_baud_sweep_stays_bounded() {
        assert!(
            PROBE_BAUDS.len() <= 6,
            "the baud sweep grew to {} rates; each one lengthens a FAILED detect on every port",
            PROBE_BAUDS.len()
        );
    }

    /// The sweep's worst case is "nothing answers", and it is paid on every failed detect. This
    /// pins the cost so a future addition is a deliberate choice rather than a silent slowdown.
    #[test]
    fn the_seeded_sweep_stays_bounded() {
        let probes: usize = COMMON_CAT_MODELS.iter().map(|(_, _, b)| b.len()).sum();
        assert!(
            probes <= 30,
            "seeded probes grew to {probes}; each one lengthens a FAILED detect. Trim the \
             interface-cable block before adding more."
        );
    }

    fn usb(port: &str, product: &str, mfr: &str) -> UsbPort {
        UsbPort {
            port_name: port.to_string(),
            vid: 0,
            pid: 0,
            product: product.to_string(),
            manufacturer: mfr.to_string(),
        }
    }

    #[test]
    fn plausible_freq_accepts_ham_bands_rejects_junk() {
        assert!(is_plausible_cat_freq(14_074_000)); // 20 m
        assert!(is_plausible_cat_freq(144_200_000)); // 2 m
        assert!(!is_plausible_cat_freq(0)); // dead port
        assert!(!is_plausible_cat_freq(50)); // garbage
        assert!(!is_plausible_cat_freq(2_000_000_000)); // out of range
    }

    #[test]
    fn native_usb_rig_resolves_its_own_model_ignoring_the_fallback() {
        let cands = candidates_from(&[usb("COM5", "IC-705", "Icom Inc.")], 1, &[]);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].port_name, "COM5");
        assert!(
            cands[0].model > 4,
            "resolved a real Hamlib model, not a built-in"
        );
        assert!(cands[0].model_name.contains("705"));
    }

    /// The wizard's second-radio probe: radio 1's port is OWNED (its live daemon holds it),
    /// so it must produce no candidates at all — every one would burn a full no-answer baud
    /// ladder before the sweep reached the port that can answer.
    #[test]
    fn an_excluded_port_produces_no_candidates() {
        let ports = [
            usb("COM5", "FT-991A", "Yaesu"),
            usb("COM7", "FT-991A", "Yaesu"),
        ];
        let cands = candidates_from(&ports, 0, &["COM5".to_string()]);
        assert!(!cands.is_empty(), "the free port still probes");
        assert!(
            cands.iter().all(|c| c.port_name == "COM7"),
            "no candidate may target the owned port: {cands:?}"
        );
        // Case-insensitive: Windows port names arrive in either case.
        let cands = candidates_from(&ports, 0, &["com5".to_string()]);
        assert!(cands.iter().all(|c| c.port_name == "COM7"));
    }

    #[test]
    fn bridge_chip_rig_uses_the_fallback_model() {
        let cands = candidates_from(
            &[usb("COM3", "CP2102 USB to UART Bridge", "Silicon Labs")],
            3073,
            &[],
        );
        assert_eq!(cands.len(), 1);
        assert_eq!(
            cands[0].model, 3073,
            "no model in the product → operator's configured model"
        );
    }

    /// An IC-7610's two ports both resolve to model 3078 — without the CI-V-side sort, an
    /// unlucky enumeration order spends the whole baud sweep on the dead "Serial Port B"
    /// twin before ever touching the real CI-V port.
    #[test]
    fn the_civ_marked_port_is_probed_before_its_dead_twin() {
        let cands = candidates_from(
            &[
                usb("COM4", "IC-7610 Serial Port B", "Icom Inc."),
                usb("COM3", "IC-7610 Serial Port A (CI-V)", "Icom Inc."),
            ],
            0,
            &[],
        );
        assert_eq!(cands.first().map(|c| c.port_name.as_str()), Some("COM3"));
        assert_eq!(cands.last().map(|c| c.port_name.as_str()), Some("COM4"));
    }

    #[test]
    fn unmatched_port_with_no_fallback_seeds_common_models() {
        // A bridge chip with no configured model used to be dropped (Auto-test found nothing until
        // a model was picked — the FTdx10 chicken-and-egg). Now it seeds the common rigs on that
        // port so the probe can still find it.
        let cands = candidates_from(
            &[usb("COM3", "CP2102 USB to UART Bridge", "Silicon Labs")],
            0,
            &[],
        );
        // One candidate per (model, seed-baud) pair — the Icom models carry two bauds each.
        let expected: usize = COMMON_CAT_MODELS.iter().map(|(_, _, b)| b.len()).sum();
        assert_eq!(cands.len(), expected);
        assert!(cands.iter().all(|c| c.port_name == "COM3"));
        assert!(cands.iter().any(|c| c.model == 1042)); // FTDX10 is seeded
                                                        // The 7610 is now seeded at both 115200 and 19200 so a non-default CI-V baud still connects.
        assert!(cands
            .iter()
            .any(|c| c.model == 3078 && c.baud == Some(115200)));
        assert!(cands
            .iter()
            .any(|c| c.model == 3078 && c.baud == Some(19200)));
    }
}
