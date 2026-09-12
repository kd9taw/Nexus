//! Shared Field Day display construction. Native snapshots and the bounded
//! Remote query read the same log, rules, score and club mirror; no mode changes.
use super::{now_unix_secs, Engine, Mode};
use crate::dto::{FieldDayQso, FieldDayStatus};
use crate::engine::upload_legs;

impl Engine {
    pub(super) fn field_day_status(&self) -> Option<FieldDayStatus> {
        if !self.settings.fd_active {
            return None;
        }
        let Mode::FieldDay { station, running } = &self.mode else {
            return None;
        };
        let log = &station.log;
        // The SESSION's ruleset — the only lookup that can name a contest `FdEvent` has
        // no arm for. `fd_rules::ruleset(log.event, ..)` was right while Field Day was the
        // only contest; its else-branch is ARRL FD, so every QSO party was scored, duped
        // and headed under Field Day's rules with nothing saying so.
        let rs = log.ruleset();
        // `scored` is the total AFTER the post-multiplier and the multipliers: for both
        // Field Day events that is the power-tier total it has always been (neither
        // declares a multiplier), and for a QSO party it is points × mults.
        let (qso_pts, _powered, mult_count, scored) = rs
            .scoring
            .score(log.score_rows(), self.settings.fd_power_mult);
        let bonus = rs.bonus_points(&self.settings.fd_bonuses);
        // The exchange and the role this session is running — read ONCE here, so the
        // strip's boxes, the sent display and the multiplier boards cannot disagree
        // about which role the operator is in.
        let spec = log.session.exchange;
        let role = log.session.role();
        // The running-or-next event window, from the rules data (the
        // banner/countdown's single source — no TS date math).
        let event_window = rs.next_or_running(now_unix_secs());
        Some(FieldDayStatus {
            running: *running,
            state: format!("{:?}", station.state),
            dxcall: station.dxcall.clone(),
            qso_count: log.qso_count(),
            sections: log.sections(),
            worked_sections: log.worked_sections(),
            points: qso_pts,
            // ⭐ How many multipliers this log has earned, by the ruleset's own
            // rules and scopes. 0 for an event that has no such concept, which
            // is both Field Day events.
            mult_count,
            // ⭐ The RULES-FILE event id, off the session — `"tnqp"`, not the
            // two-arm Field Day enum this read before, which reported every QSO
            // party as ARRL Field Day to every surface downstream.
            event: log.session.event_id.clone(),
            powered_points: scored,
            bonus_points: bonus,
            total_score: scored + bonus,
            event_start_unix: event_window.start_unix,
            event_end_unix: event_window.end_unix,
            rules_year: rs.rules_year,
            // The ruleset says what its own score leaves out; the UI renders
            // it beside the total. Empty for both Field Day events.
            score_note_key: rs.score_note_key.to_string(),
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
                    class: q.class().to_string(),
                    section: q.section().to_string(),
                    band: q.band.clone(),
                    mode: q.mode.clone(),
                    submode: q.submode.clone(),
                    when_unix: q.when_unix,
                    // ⭐ THE ROW'S OWN SENT EXCHANGE (§3.3). Rendered from the
                    // row by the one function that renders one, so an emitter
                    // looping over these rows has the right value in hand and
                    // no reason to reach out to the session.
                    mex: tempo_core::contest::sent_exchange_string(q, spec),
                })
                .collect(),
            club: self.fd_club_dto(log),
            upload: crate::dto::FdUploadDto {
                enabled: log.session.upload.enabled,
                destinations: log.session.upload.destinations.clone(),
                available: upload_legs::IDS.iter().map(|s| s.to_string()).collect(),
                hint: tempo_core::contest::UPLOAD_CLUBLOG_SWEEP_HINT.to_string(),
            },
            // ⭐ THE DYNAMIC ENTRY STRIP'S SHAPE (§9). The strip renders Call
            // plus one box per slot HERE, in this order — never a hardcoded
            // Class/Section pair. Field Day's role receives exactly those two,
            // so the shipped strip is what this produces.
            receives: role
                .receives
                .iter()
                .filter_map(|k| spec.field(k))
                .map(|f| crate::dto::FdFieldDto {
                    key: f.key.to_string(),
                    kind: crate::dto::field_kind_tag(&f.kind).to_string(),
                    required: f.required,
                    domain: match f.kind {
                        tempo_core::contest::FieldKind::Enum { domain } => {
                            Some(domain.id.to_string())
                        }
                        _ => None,
                    },
                })
                .collect(),
            // The read-only sent display, as a VECTOR — §3.3 mechanism 2. It
            // describes the session, which is what is about to go on the air;
            // nothing that describes a row already logged may read it.
            composing: log
                .session
                .my_exchange
                .iter()
                .map(|v| crate::dto::FdFieldValueDto {
                    key: v.key.to_string(),
                    raw: v.raw.clone(),
                    domain: v.domain.map(|d| d.to_string()),
                })
                .collect(),
            role: role.id.to_string(),
            boards: tempo_core::contest::boards(&rs.scoring, spec, role)
                .into_iter()
                .map(|b| crate::dto::FdBoardDto {
                    id: b.id.to_string(),
                    slot: b.slot.to_string(),
                    domain: b.domain.map(|d| d.to_string()),
                    scope: crate::dto::mult_scope_tag(b.scope).to_string(),
                    worked: log.worked_values(b.slot),
                })
                .collect(),
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
        // `myexch` is gone — one sent exchange for a whole log could not survive a
        // mobile contest (§3.3). The session's composing vector is its replacement, and
        // it is what must be bounded now.
        for v in &log.session.my_exchange {
            check(&v.raw)?;
        }
        if let Some(s) = &station.dxcall {
            check(s)?;
        }
        for q in log.qsos() {
            // `class`/`section` became per-row ACCESSORS when the log started carrying
            // both sides of the exchange per row (§3.3) — they are no longer fields.
            for s in [&q.call, &q.band, &q.mode, &q.submode] {
                check(s)?;
            }
            check(q.class())?;
            check(q.section())?;
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
        // `myexch.class` was the sent CLASS when a log had one exchange for its whole
        // life. The session's `my_exchange` vector replaced it (§3.3) — same intent
        // here: one oversized sent field must trip the bound.
        if let Some(v) = station.log.session.my_exchange.first_mut() {
            v.raw = "x".repeat(1025);
        } else {
            station
                .log
                .session
                .my_exchange
                .push(tempo_core::contest::FieldValue {
                    key: "class",
                    raw: "x".repeat(1025),
                    domain: None,
                });
        }
        assert_eq!(
            e.bounded_field_day_status().unwrap_err(),
            "applicationTooLarge"
        );
        e.settings.fd_active = false;
        assert!(e.bounded_field_day_status().unwrap().is_none());
        assert!(e.snapshot().field_day.is_none());
    }
}
