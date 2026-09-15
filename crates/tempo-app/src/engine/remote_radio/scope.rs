//! Remote rig-scope settings: the native panadapter's span, reference level and position.
//!
//! These are receive-display one-shots. Admission applies the SAME verbs the cockpit's scope
//! controls call locally (`request_scope_span`, `request_scope_ref`, `request_yaesu_scope_mode`,
//! `set_flex_pan_span`, `set_flex_pan_ref`), which the radio loop or the FlexSpectrum worker then
//! applies exactly as for a local click. Nothing here arms, keys, retunes the dial or saves Settings.
//!
//! A browser cannot see which scope the station runs, so the station judges the family. The caller
//! names the native panadapter the station's configuration starts a worker for (the rig model and
//! opt-in the radio loop reads), and the FT-710 family is the scope MODE code the loop last read back
//! from the radio. A setting that no live family takes is refused rather than queued for a radio
//! that cannot apply it. The FT-710 span and position are written from the loop's scope reconcile
//! with no keyed guard of their own, so nothing here is admitted while the transmitter is owned or a
//! tune carrier is up.
use super::*;

/// The native panadapter the station's configuration runs a worker for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeFamily {
    IcomCiv,
    Flex,
    None,
}

/// One scope setting, already resolved to what the local verb takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteScope {
    /// Icom CI-V: the ± half-width. FT-710: half of the full span (the loop doubles it back).
    Span(u32),
    /// Icom CI-V reference level, in tenths of a dB.
    Ref(i32),
    /// The FT-710 `SS` P3 mode code, resolved beside the radio from the code it reports.
    Position(u8),
    /// FlexRadio pan bandwidth, in Hz.
    PanSpan(u32),
    /// FlexRadio pan reference level in dBm; `None` is auto.
    PanRef(Option<i32>),
}

/// The Icom CI-V span chips the cockpits offer (± half-width, Hz).
pub const ICOM_SCOPE_SPANS_HZ: [u32; 7] = [2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000];
/// The FT-710 span rungs the cockpits offer, as half-widths (Hz): 1 kHz to 1 MHz full span.
pub const YAESU_SCOPE_HALF_SPANS_HZ: [u32; 10] = [
    500, 1_000, 2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
];
/// Every FT-710 `SS` P3 mode code: CENTER, CURSOR and FIX in 3DSS, W/F EXPAND and W/F NORMAL.
const YAESU_MODE_CODES: [u8; 9] = *b"01236947A";

impl Engine {
    pub fn queue_remote_scope(
        &mut self,
        setting: RemoteScope,
        family: ScopeFamily,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        // A fresh, unkeyed CAT link to the radio the page displayed.
        self.remote_radio_link(connection)?;
        if self.tx_owner().is_some() || self.tuning {
            return Err(Reason::StationBusy);
        }
        let ft710 = self.scope_mode_code.is_some();
        match setting {
            RemoteScope::Span(hz) => {
                let icom = family == ScopeFamily::IcomCiv;
                if !icom && !ft710 {
                    return Err(Reason::HardwareUnavailable);
                }
                if !(icom && ICOM_SCOPE_SPANS_HZ.contains(&hz))
                    && !(ft710 && YAESU_SCOPE_HALF_SPANS_HZ.contains(&hz))
                {
                    return Err(Reason::InvalidAction);
                }
                self.request_scope_span(hz);
            }
            RemoteScope::Ref(tenths) => {
                if family != ScopeFamily::IcomCiv {
                    return Err(Reason::HardwareUnavailable);
                }
                if !(-200..=200).contains(&tenths) {
                    return Err(Reason::InvalidAction);
                }
                self.request_scope_ref(tenths);
            }
            RemoteScope::Position(code) => {
                if !ft710 {
                    return Err(Reason::HardwareUnavailable);
                }
                if !YAESU_MODE_CODES.contains(&code) {
                    return Err(Reason::InvalidAction);
                }
                self.request_yaesu_scope_mode(code);
            }
            RemoteScope::PanSpan(hz) => {
                if family != ScopeFamily::Flex {
                    return Err(Reason::HardwareUnavailable);
                }
                if !(5_000..=14_000_000).contains(&hz) {
                    return Err(Reason::InvalidAction);
                }
                self.set_flex_pan_span(f64::from(hz));
            }
            RemoteScope::PanRef(reference) => {
                if family != ScopeFamily::Flex {
                    return Err(Reason::HardwareUnavailable);
                }
                if reference.is_some_and(|dbm| !(-160..=20).contains(&dbm)) {
                    return Err(Reason::InvalidAction);
                }
                self.set_flex_pan_ref(reference);
            }
        }
        Ok(())
    }
}
