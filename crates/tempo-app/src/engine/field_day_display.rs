//! Shared Field Day display construction. Native snapshots and the bounded
//! Remote query read the same log, rules, score and club mirror; no mode changes.
use super::{now_unix_secs, Engine, Mode};
use crate::dto::{FieldDayQso, FieldDayStatus};

impl Engine {
    pub(super) fn field_day_status(&self) -> Option<FieldDayStatus> {
        if !self.settings.fd_active {
            return None;
        }
        let Mode::FieldDay { station, running } = &self.mode else {
            return None;
        };
        let log = &station.log;
        let rs = tempo_core::fd_rules::ruleset(log.event, tempo_core::fd_rules::CURRENT_RULES_YEAR);
        let (qso_pts, powered) = rs.scoring.qso_and_powered(log, self.settings.fd_power_mult);
        let bonus = rs.bonus_points(&self.settings.fd_bonuses);
        // The running-or-next event window, from the rules data (the
        // banner/countdown's single source — no TS date math).
        let event_window = rs.next_or_running(now_unix_secs());
        Some(FieldDayStatus {
            my_class: log.myexch.class.clone(),
            my_section: log.myexch.section.clone(),
            running: *running,
            state: format!("{:?}", station.state),
            dxcall: station.dxcall.clone(),
            qso_count: log.qso_count(),
            sections: log.sections(),
            worked_sections: log.worked_sections(),
            points: qso_pts,
            event: if matches!(log.event, tempo_core::fieldday::FdEvent::WinterFd) {
                "wfd".into()
            } else {
                "arrlfd".into()
            },
            powered_points: powered,
            bonus_points: bonus,
            total_score: powered + bonus,
            event_start_unix: event_window.start_unix,
            event_end_unix: event_window.end_unix,
            rules_year: rs.rules_year,
            rules_generated: tempo_core::fd_rules::active_generated().to_string(),
            // The effectively-ON assistance sources, by their display
            // labels — the advisory UI's single source (never re-derived).
            assistance_on: self
                .settings
                .assistance_sources()
                .iter()
                .filter(|&&(_, on)| on)
                .map(|&(label, _)| label.to_string())
                .collect(),
            log: log
                .qsos()
                .iter()
                .map(|q| FieldDayQso {
                    call: q.call.clone(),
                    class: q.class.clone(),
                    section: q.section.clone(),
                    band: q.band.clone(),
                    mode: q.mode.clone(),
                    submode: q.submode.clone(),
                    when_unix: q.when_unix,
                })
                .collect(),
            club: self.fd_club_dto(log),
        })
    }

    /// Bound row counts and text before cloning the existing display DTO. The
    /// ordinary native snapshot keeps its established complete-log behavior.
    pub fn bounded_field_day_status(&self) -> Result<Option<FieldDayStatus>, &'static str> {
        if !self.settings.fd_active {
            return Ok(None);
        }
        let Mode::FieldDay { station, .. } = &self.mode else {
            return Ok(None);
        };
        let log = &station.log;
        if log.qso_count() > 2048
            || (self.fd_sync_enabled()
                && (self.fd_mirror.dupes.len() > 4096
                    || self.fd_mirror.board.len() > 128
                    || self.fd_mirror.sections.len() > 256))
        {
            return Err("applicationTooLarge");
        }
        let mut bytes = 0;
        let mut check = |s: &str| {
            bytes += s.len();
            if s.len() > 1024 || bytes > 128 * 1024 {
                Err("applicationTooLarge")
            } else {
                Ok(())
            }
        };
        for s in [&log.myexch.class, &log.myexch.section] {
            check(s)?;
        }
        if let Some(s) = &station.dxcall {
            check(s)?;
        }
        for q in log.qsos() {
            for s in [&q.call, &q.class, &q.section, &q.band, &q.mode, &q.submode] {
                check(s)?;
            }
        }
        if self.fd_sync_enabled() {
            for s in [&self.fd_mirror.event, &self.fd_mirror.host_call] {
                check(s)?;
            }
            if let Some(s) = &self.fd_mirror.last_error {
                check(s)?;
            }
            for (call, band, mode) in &self.fd_mirror.dupes {
                for s in [call, band, mode] {
                    check(s)?;
                }
            }
            for r in &self.fd_mirror.board {
                for s in [&r.pos, &r.name, &r.band, &r.mode, &r.op] {
                    check(s)?;
                }
            }
            for s in &self.fd_mirror.sections {
                check(s)?;
            }
        }
        Ok(self.field_day_status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn engine(event: &str, count: usize) -> Engine {
        let s = crate::settings::Settings {
            fd_active: true,
            fd_event: event.into(),
            fd_class: "1D".into(),
            fd_section: "EMA".into(),
            fd_power_mult: 2,
            ..Default::default()
        };
        let mut e = Engine::with_settings(s);
        e.restore_field_day_if_enabled();
        let Mode::FieldDay { station, .. } = &mut e.mode else {
            panic!("active")
        };
        for i in 0..count {
            assert!(station.log.log_mode_at(
                &format!("K1T{i}"),
                "2A",
                if i % 2 == 0 { "WI" } else { "EMA" },
                if i % 2 == 0 { "PH" } else { "CW" },
                i as u64,
                1000
            ));
        }
        e
    }
    #[test]
    fn bounded_capture_retains_the_native_event_score_and_complete_log() {
        for event in ["arrlfd", "wfd"] {
            for count in [0, 512, 2048] {
                let e = engine(event, count);
                let before = e.field_day_log_adif();
                let native = e.snapshot().field_day.unwrap();
                let captured = e.bounded_field_day_status().unwrap().unwrap();
                assert_eq!(
                    serde_json::to_value(&captured).unwrap(),
                    serde_json::to_value(native).unwrap()
                );
                assert_eq!(captured.qso_count, count);
                assert_eq!(captured.log.len(), count);
                assert_eq!(captured.event, event);
                assert_eq!(captured.sections, if count == 0 { 0 } else { 2 });
                assert_eq!(e.field_day_log_adif(), before);
            }
        }
    }

    #[test]
    fn club_capture_preserves_native_rows_and_ignores_a_disabled_mirror() {
        let mut e = engine("arrlfd", 2);
        e.settings.fd_join_addr = "127.0.0.1:7878".into();
        e.fd_mirror.event = "Club Field Day".into();
        e.fd_mirror.host_call = "W1AW".into();
        e.fd_mirror.connected = true;
        e.fd_mirror.board = (0..128)
            .map(|i| tempo_net::fdsync::WireBoardRow {
                pos: i.to_string(),
                name: format!("Position {i}"),
                band: "20m".into(),
                mode: "CW".into(),
                op: "W1AW".into(),
                qsos: 2,
                uniq: 2,
                rate: 1,
                age: 17,
            })
            .collect();
        let before = serde_json::to_value(e.snapshot().field_day).unwrap();
        let value = e.bounded_field_day_status().unwrap().unwrap();
        assert_eq!(serde_json::to_value(&value).unwrap(), before);
        assert_eq!(value.club.unwrap().board.len(), 128);
        e.fd_mirror.board.push(e.fd_mirror.board[0].clone());
        assert_eq!(
            e.bounded_field_day_status().unwrap_err(),
            "applicationTooLarge"
        );
        e.fd_mirror.board.pop();
        for row in &mut e.fd_mirror.board {
            row.name = "x".repeat(1024);
        }
        assert_eq!(
            e.bounded_field_day_status().unwrap_err(),
            "applicationTooLarge"
        );
        e.fd_mirror.board.clear();
        e.fd_mirror.dupes = (0..4097)
            .map(|i| (format!("K1T{i}"), "20m".into(), "CW".into()))
            .collect();
        assert_eq!(
            e.bounded_field_day_status().unwrap_err(),
            "applicationTooLarge"
        );
        e.settings.fd_join_addr.clear();
        e.fd_mirror.last_error = Some("x".repeat(1025));
        assert!(e
            .bounded_field_day_status()
            .unwrap()
            .unwrap()
            .club
            .is_none());
        assert!(e.snapshot().field_day.unwrap().club.is_none());
        assert_eq!(e.snapshot().field_day.unwrap().qso_count, 2);
    }
    #[test]
    fn size_refusal_does_not_truncate_or_change_the_native_event() {
        let e = engine("arrlfd", 2049);
        assert_eq!(
            e.bounded_field_day_status().unwrap_err(),
            "applicationTooLarge"
        );
        assert_eq!(e.snapshot().field_day.unwrap().qso_count, 2049);
        let mut e = engine("arrlfd", 1);
        let Mode::FieldDay { station, .. } = &mut e.mode else {
            panic!("active")
        };
        station.log.myexch.class = "x".repeat(1025);
        assert_eq!(
            e.bounded_field_day_status().unwrap_err(),
            "applicationTooLarge"
        );
        e.settings.fd_active = false;
        assert!(e.bounded_field_day_status().unwrap().is_none());
        assert!(e.snapshot().field_day.is_none());
    }
}
