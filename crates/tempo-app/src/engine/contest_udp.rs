//! ⭐ **A contest contact for the WSJT-X UDP sink, complete** — what WSJT-X sends for every
//! contest QSO it logs (`MainWindow::acceptQSO`: a `QSOLogged` with the grid, both reports, the
//! power, the frequency on the air, both times, the operator and both exchanges, then a
//! `LoggedADIF` with the record).
//!
//! The slot loop's contest emitter used to build its `QSOLogged` from the snapshot's display row,
//! which carries none of that: no grid, no reports, the bare dial, the slot's clock as both start
//! and end, the station's call as the operator, and a received exchange read off Field Day's two
//! slots, blank in every other contest. This hands it the row itself, as the Logbook will hold it
//! ([`tempo_core::contest::merge::logged_contact`]).
//!
//! **Read only**: nothing here keys, unkeys, plans or times a transmission, and nothing is written.

use super::{Engine, Mode};
use tempo_core::contest::merge::LoggedContact;

impl Engine {
    /// The contest contact logged as row `seq`, complete, for the WSJT-X UDP sink — `None` when
    /// no contest runs or no row has that seq.
    ///
    /// Beside the record a Merge would write, two things only the live session knows: when the
    /// FT sequencer began the contact (its start time), and, where the exchange carried no grid,
    /// the grid the station was heard with — WSJT-X logs the grid off the CQ it answered — in the
    /// record too, so both datagrams of one contact say the same thing.
    pub fn contest_logged_contact(&self, seq: u64) -> Option<LoggedContact> {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return None;
        };
        let q = station.log.qsos().iter().find(|q| q.seq == seq)?;
        let mut c = tempo_core::contest::merge::logged_contact(
            &station.log,
            &self.settings.fd_position_id,
            q,
            station.started_unix_of(seq),
        );
        if c.record.grid.as_deref().is_none_or(|g| g.trim().is_empty()) {
            c.record.grid = self.roster_grid(&q.call).map(str::to_string);
        }
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_day() -> Engine {
        let mut e = Engine::with_settings(crate::settings::Settings {
            fd_active: true,
            fd_event: "arrlfd".into(),
            fd_class: "2B".into(),
            fd_section: "EMA".into(),
            ..Default::default()
        });
        e.restore_field_day_if_enabled();
        e
    }

    /// ⭐ Field Day's exchange carries no grid, and WSJT-X logs the one the station was heard
    /// with: the grid the roster holds for the call goes into the record, so the `QSOLogged` and
    /// the `LoggedADIF` built from it both carry it. CONTROL: a call never heard has none.
    #[test]
    fn a_field_day_contact_takes_the_grid_it_was_heard_with() {
        let mut e = field_day();
        let heard = modes::Decode {
            message: "CQ FD W1AW FN31".into(),
            sync: 1.0,
            snr: -5,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            raw: None,
            unchecked: false,
            mode: None,
        };
        e.app.inbox.roster.observe(&heard, 0);
        assert!(e
            .fd_log_manual_submode("W1AW", "1D", "CT", "DIG", "FT8")
            .unwrap());
        assert!(e
            .fd_log_manual_submode("K9ABC", "1D", "IL", "DIG", "FT8")
            .unwrap());
        let seqs: Vec<u64> = e
            .snapshot()
            .field_day
            .unwrap()
            .log
            .iter()
            .map(|q| q.seq)
            .collect();
        let grid = |seq| e.contest_logged_contact(seq).unwrap().record.grid;
        assert_eq!(grid(seqs[0]).as_deref(), Some("FN31"));
        assert_eq!(grid(seqs[1]), None);
    }
}
