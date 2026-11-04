//! A contact ANOTHER PROGRAM logged, on its way into the Nexus log ([`crate::log_intake`]): what
//! the station stamps on it before the import, and where it goes after.
//!
//! ⛔ **Not through the log funnel** ([`Engine::log_qso_inner`]). The funnel is the contact the
//! operator just made in Nexus: it ends the FT rescue window (`stalled_qso`), ticks
//! `logged_tick` (the cockpit clears its DX boxes on it), answers JTTY's prompt and lets go of a
//! contact a band change set aside. A contact WSJT-X or the S.A.T. box logged is none of that, and
//! one arriving while Nexus's own FT exchange holds the same call must leave that exchange alone.
//! So the contact goes in by the import ([`crate::logwrite::intake_contact`]), and only these
//! pieces of the funnel are borrowed, each called as it is and none changed:
//!
//! - the funnel's 300 s duplicate check ([`crate::station::StationCore::is_duplicate`]), so a
//!   contact the operator also logged in Nexus is not written twice;
//! - the station call, when the record names none;
//! - the running POTA/SOTA activation and the station profile, only when the record carries no
//!   reference of its own (a park activator working the S.A.T. box wants the park);
//! - the service uploads, held while contest mode is on, exactly as the funnel queues them.
//!
//! ⛔ **Never the local loggers** (WSJT-X UDP out, HRD, N1MM, DXKeeper): the program that sent
//! the contact may be one of them, or listening beside them, and a contact sent back would come
//! in again. The upload worker leaves DXKeeper out for a contact that carries the "via" mark
//! ([`crate::log_intake::from_another_program`]).

use super::{upload_legs, Engine, PendingUpload, UploadOrigin};
use tempo_core::logbook::QsoRecord;

impl Engine {
    /// Ready a contact another program logged for the import: in the log's one form, then the
    /// funnel's duplicate check. `true` when the log already holds it by that check (nothing is
    /// stamped then). Otherwise the station call when the record names none, and the running
    /// activation and station profile when it carries no reference of its own.
    pub(crate) fn intake_ready(&self, rec: &mut QsoRecord) -> bool {
        tempo_core::logbook::canonical::contact(rec);
        if self.station.is_duplicate(rec) {
            return true;
        }
        if rec.station_callsign.is_none() {
            rec.station_callsign = self.station_callsign_now();
        }
        if !carries_own_reference(rec) {
            self.stamp_activation(rec);
            self.stamp_station_profile(rec);
        }
        false
    }

    /// A contact another program logged, now in the log, queued for the service uploads as the
    /// funnel queues the operator's own: held while contest mode is on, live otherwise.
    pub(crate) fn intake_queue_upload(&mut self, rec: QsoRecord) {
        if self.settings.fd_active {
            self.hold_contest_upload(rec);
        } else {
            self.station.enqueue_upload(PendingUpload {
                rec,
                origin: UploadOrigin::Live,
                legs: upload_legs::ALL,
                attempts: 0,
                retry_after_unix: 0,
                corrected: false,
            });
        }
    }
}

/// Whether the record names a park, a summit or another reference of the operator's own: then
/// neither the running activation nor the station profile is stamped on it.
fn carries_own_reference(rec: &QsoRecord) -> bool {
    rec.ota.my_ref.is_some()
        || rec.ota.my_program.is_some()
        || rec.extra.iter().any(|(k, _)| {
            matches!(
                k.as_str(),
                "MY_SIG" | "MY_SIG_INFO" | "MY_POTA_REF" | "MY_SOTA_REF" | "MY_WWFF_REF"
            )
        })
}

#[cfg(test)]
mod tests;
