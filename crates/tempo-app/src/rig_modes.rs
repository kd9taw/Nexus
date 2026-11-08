//! The modes a radio model's CAT backend can express, as that backend itself says.
//!
//! # Why
//!
//! Nexus commands a DATA submode for every soundcard mode (`PKTUSB`/`PKTLSB`, `PKTFM` for an
//! SSTV picture on FM), and it used to send that word to every radio. Not every Hamlib backend
//! has one. Model 2056 (SDR Console, a TS-2000-family backend) lists
//! `AM CW USB LSB RTTY FM CWR RTTYR` and no PKT mode, and it does not refuse `M PKTUSB`: it
//! puts `MDD;` on the wire, which is not a TS-2000 mode. SDR Console raised an error window at
//! every FT8 key-down, because the mode set goes out on the slot boundary just before `TX;`.
//!
//! So the rule is the backend's own word, never a table: a mode the backend does not list is
//! not commanded when the same emission has another name the backend does list. The SSB DATA
//! submodes become the plain sideband, exactly as the operator's "Data modes use plain SSB"
//! switch makes them (`Settings::plain_ssb_if_configured`), and CW-reverse becomes CW
//! (the same emission; only the receiver's side differs). Modes with no such twin (AM, FM,
//! the rig's own RTTY) are still commanded and left to the radio loop's refusal ladder, which
//! tells the operator what to do.
//!
//! # What the list can be trusted for, and what it cannot
//!
//! Hamlib's printed `Mode list` is the union of the backend's declared receive ranges
//! (`dumpcaps.c`, `rs->mode_list`), and `rig_set_mode` never checks a mode against it: each
//! backend's own `set_mode` decides. So the list is a declaration, and it UNDER-reports in places.
//! Read against the Hamlib 4.7.1 source:
//!
//! - **Icom (every CI-V backend).** `icom_set_mode` turns `PKTUSB` into USB with the data flag on
//!   whenever the backend's private caps set `data_mode_supported` (`icom.c`; the IC-9700 and
//!   IC-756PRO II/III among them), and into plain USB otherwise (`frame.c` `rig2icom_mode`). The
//!   IC-9700's list prints no PKT mode at all, and FT8 through it works in USB-D. An Icom
//!   backend therefore never puts an invalid mode on the wire, and is never recorded here
//!   (`tempo-audio` leaves it unknown).
//! - **`PKTFM`.** Yaesu's newcat maps it to `A` (DATA-FM) for every newcat radio while the
//!   FT-891's list omits it, and the TS-990S's own Kenwood mode table carries it (`E`) while its
//!   list omits it. So a missing `PKTFM` (Hamlib prints it `FM-D`) is not evidence of anything,
//!   and is not acted on.
//! - **`PKTUSB`, `PKTLSB` and `CWR`** on every other backend: an unlisted one was refused or
//!   garbled on each backend checked (the Kenwood table sends `PKTUSB` as `D`, giving SDR
//!   Console's `MDD;`). And landing on the plain twin costs nothing a working radio had: it is
//!   what the refusal ladder already ended on, after sending the bad word first.
//!
//! # Who fills it
//!
//! `tempo-audio`, when it launches Hamlib's `rigctld` for a model: from that same binary's
//! `-u` (dump caps) output, read once per model. A model served by anything else (Nexus's own
//! CI-V, Flex, TCI or Icom network client, OmniRig) is removed, because Hamlib's list is not
//! what that daemon speaks.
//!
//! # Unknown answers "not lacking"
//!
//! A model with no entry (caps not read, the dump failed, an Icom, a NET rigctl attach whose
//! remote model Nexus never saw) lacks nothing, so everything is commanded as before. Only a list
//! Hamlib actually printed can withhold a mode.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// Model number → the backend's mode words, upper case, as Hamlib prints them.
fn known() -> MutexGuard<'static, HashMap<u32, Vec<String>>> {
    static KNOWN: OnceLock<Mutex<HashMap<u32, Vec<String>>>> = OnceLock::new();
    KNOWN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Bumped on every change, so a caller that computed a mode word before a change can tell that
/// it has to compute it again ([`generation`]).
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// One mode under one name, whichever spelling it arrived in.
///
/// ⚠️ HAMLIB PRINTS SOME MODES UNDER A DIFFERENT NAME FROM THE ONE NEXUS SENDS. 4.6.5 parses
/// `PKTFM` and prints it `FM-D` (and `PKTAM` as `AM-D`), in the caps dump and in `m` alike, so
/// the FT-710's list reads `… PKTLSB PKTUSB FM-D …`. Compared as bare strings, that radio would
/// "lack" `PKTFM` and lose FM-D for an SSTV picture. The other spellings are the ones Nexus
/// already reads back from radios (`civ::commands`, `fallback_sideband` in tempo-audio).
fn canonical(mode: &str) -> String {
    let m = mode.trim().to_ascii_uppercase();
    match m.as_str() {
        "FM-D" | "PKT-FM" => "PKTFM",
        "AM-D" | "PKT-AM" => "PKTAM",
        "USB-D" | "DATA-U" | "PKT-U" => "PKTUSB",
        "LSB-D" | "DATA-L" | "PKT-L" => "PKTLSB",
        "CW-R" => "CWR",
        "RTTY-R" => "RTTYR",
        _ => return m,
    }
    .to_string()
}

/// Record what the backend serving `model` can express: `Some(list)` from its own caps, or
/// `None` to forget it (served by something else now, or unreadable).
pub fn record(model: u32, modes: Option<Vec<String>>) {
    let mut k = known();
    let changed = match modes {
        Some(list) => {
            let list: Vec<String> = list.iter().map(|m| canonical(m)).collect();
            k.insert(model, list.clone()) != Some(list)
        }
        None => k.remove(&model).is_some(),
    };
    if changed {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Is there a list for `model`?
pub fn is_known(model: u32) -> bool {
    known().contains_key(&model)
}

/// The modes whose ABSENCE from a list is acted on (see the module header for why `PKTFM` is not
/// among them).
const ABSENCE_TRUSTED: [&str; 3] = ["PKTUSB", "PKTLSB", "CWR"];

/// Does `model`'s backend lack `mode`, so that Nexus should command its plain twin instead?
/// `false` when the model's list is unknown, and for any mode outside [`ABSENCE_TRUSTED`].
pub fn lacks(model: u32, mode: &str) -> bool {
    let mode = canonical(mode);
    ABSENCE_TRUSTED.contains(&mode.as_str())
        && known()
            .get(&model)
            .is_some_and(|list| !list.contains(&mode))
}

/// A counter that moves whenever any model's list changes.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Model numbers no real radio has: the registry is process-wide, and tests run in parallel.
    const A: u32 = 990_101;
    const B: u32 = 990_102;

    #[test]
    fn an_unknown_model_lacks_nothing() {
        assert!(!is_known(A));
        assert!(!lacks(A, "PKTUSB"));
        assert!(!lacks(A, "CWR"));
    }

    #[test]
    fn a_known_model_lacks_only_what_its_list_leaves_out() {
        record(B, Some(vec!["usb".into(), "LSB".into(), "CW".into()]));
        assert!(is_known(B));
        assert!(lacks(B, "PKTUSB"));
        assert!(lacks(B, " cwr "));
        assert!(!lacks(B, "USB"));
        assert!(!lacks(B, "cw"));
        let before = generation();
        record(B, Some(vec!["USB".into(), "LSB".into(), "CW".into()]));
        assert_eq!(generation(), before, "the same list again is no change");
        record(B, None);
        assert!(generation() > before, "forgetting it is");
        assert!(!lacks(B, "PKTUSB"), "forgotten is unknown again");
    }

    /// Hamlib 4.6.5's FT-710 list, verbatim: `PKTFM` is printed `FM-D`, and is there.
    #[test]
    fn a_mode_hamlib_prints_under_another_name_is_not_lacking() {
        const FT710: u32 = 990_103;
        let list = "AM CW USB LSB RTTY FM CWR RTTYR PKTLSB PKTUSB FM-D FMN AMN PKTFMN";
        record(FT710, Some(list.split(' ').map(String::from).collect()));
        assert!(!lacks(FT710, "PKTFM"));
        assert!(!lacks(FT710, "FM-D"));
        assert!(!lacks(FT710, "PKTUSB"));
        assert!(!lacks(FT710, "CW-R"));
    }

    /// A missing `PKTFM` is not acted on: Hamlib's lists under-report it (the FT-891's omits
    /// what newcat sends as DATA-FM). Nor is anything outside the three trusted words.
    #[test]
    fn only_the_ssb_data_submodes_and_cw_reverse_are_withheld() {
        const SDR: u32 = 990_104;
        let list = "AM CW USB LSB RTTY FM";
        record(SDR, Some(list.split(' ').map(String::from).collect()));
        assert!(lacks(SDR, "PKTUSB") && lacks(SDR, "USB-D") && lacks(SDR, "PKTLSB"));
        assert!(lacks(SDR, "CWR"));
        assert!(!lacks(SDR, "PKTFM"), "PKTFM's absence proves nothing");
        assert!(!lacks(SDR, "FM-D"));
        assert!(
            !lacks(SDR, "RTTYR"),
            "no twin of the same emission: left to the radio"
        );
        assert!(!lacks(SDR, "WFM"));
    }
}
