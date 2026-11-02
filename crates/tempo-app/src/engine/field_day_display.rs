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
        // ⭐ The TICKED menu plus what the LOG earned. ILQP's two club calls are worth
        // 100 each to whoever works them ("added to the final score"), and nothing asks
        // the operator to claim them — a screen that showed only the ticked menu would
        // be 200 points light on the number the sponsor credits.
        let bonus = rs.bonus_points(&self.settings.fd_bonuses) + log.bonus_station_points();
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
            entry_call: log.mycall.clone(),
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
            // ⭐ The ONE formula: a contest that scores by objectives (Winter Field Day) claims
            // QSO points × (OM + 1), every other contest what it always claimed.
            total_score: rs.claimed_total(qso_pts, scored, bonus, &self.settings.fd_objectives),
            objective_multiplier: (!rs.objective_menu.is_empty())
                .then(|| rs.objective_multiplier(&self.settings.fd_objectives)),
            satellite_credit: (!rs.satellite_credit).then_some(false),
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
            rst_sent_box: self
                .settings
                .contest_rst_sent_box
                .then(|| log.session.sent_rst_constant().map(str::to_string))
                .flatten(),
            log: {
                let numbers = log.qso_numbers();
                log.qsos()
                    .iter()
                    .map(|q| field_day_qso(q, rs, spec, role, numbers.get(&q.seq).copied()))
                    .collect()
            },
            // The club block, plus the generalised keys `fd_club_dto` does not carry —
            // it lives in `engine.rs` and projects the mirror to the legacy triple. The
            // unprojected set is right here on the mirror, so it is added here rather
            // than by widening a function in a file another change is holding open.
            //
            // CLUB-ONLY, exactly as the legacy `dupes` beside it: the own log already
            // ships every row's key in `log`, and a Remote capture is measured against a
            // byte bound that an unsubtracted second copy could push it past.
            club: self.fd_club_dto(log).map(|mut c| {
                let own: std::collections::HashSet<Vec<String>> =
                    log.qsos().iter().map(|q| rs.dupe_rule.key(q)).collect();
                let mut k: Vec<Vec<String>> = self
                    .fd_mirror
                    .dkeys
                    .iter()
                    .filter(|k| !own.contains(*k))
                    .cloned()
                    .collect();
                k.sort();
                c.dkeys = k;
                c
            }),
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
                .map(crate::dto::FdFieldDto::from_spec)
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
            // What `{EXCH}` keys next — the RTTY macros read it here. It describes the
            // next transmission, never a logged row (those carry `mex`).
            sent_exchange: self.contest_sent_exchange().unwrap_or_default(),
            // The same exchange as a human reads it — report included, and the ISSUED
            // serial rather than `composing`'s `"0"` placeholder. Separate from the
            // vector below because that one is also the "I moved" edit surface; see
            // `FieldDayStatus::composing_text`.
            composing_text: self.contest_composing_text().unwrap_or_default(),
            // …and as a phone contact sends it, the report at two digits.
            composing_text_phone: self.contest_composing_text_phone().unwrap_or_default(),
            // What the sponsor requires and the file exported now would leave out, read off the
            // header the export writes for the entrant's Settings; nothing is read for a contest
            // whose requirements this build does not know.
            cabrillo_missing: if rs.cabrillo_required.is_empty() {
                Vec::new()
            } else {
                log.cabrillo_missing(&self.contest_entrant())
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            },
            // A serial to give is a serial sent: what Clear test contacts starts again at 001.
            sends_serial: log.session.serial_now().is_some(),
            bands: rs.bands.iter().map(|b| b.to_string()).collect(),
            // The dupe rule's mode grouping, so the strip's badge and the engine's
            // refusal answer the same question.
            dupe_mode_groups: rs
                .dupe_rule
                .mode_class_groups
                .iter()
                .map(|g| g.iter().map(|m| m.to_string()).collect())
                .collect(),
            // ⭐ The WHOLE rule, so the strip's badge can build the key the engine will
            // refuse on instead of the `(call, band, mode class)` triple it hardcoded —
            // which is the rule for two of the eighteen shipped rulesets.
            dupe_rule: crate::dto::DupeRuleDto {
                by_call: rs.dupe_rule.by_call,
                by_band: rs.dupe_rule.by_band,
                by_mode_class: rs.dupe_rule.by_mode_class,
                by_fields: rs
                    .dupe_rule
                    .by_fields
                    .iter()
                    .map(|k| k.to_string())
                    .collect(),
                by_sent_fields: rs
                    .dupe_rule
                    .by_sent_fields
                    .iter()
                    .map(|k| k.to_string())
                    .collect(),
                mode_class_groups: rs
                    .dupe_rule
                    .mode_class_groups
                    .iter()
                    .map(|g| g.iter().map(|m| m.to_string()).collect())
                    .collect(),
                log_dupes: rs.dupe_rule.log_dupes,
            },
            location_warning: log
                .session
                .location_warning
                .as_ref()
                .map(crate::dto::LocationWarningDto::from),
            role: role.id.to_string(),
            boards: tempo_core::contest::boards(&rs.scoring, spec, role)
                .into_iter()
                .map(|b| crate::dto::FdBoardDto {
                    id: b.id.to_string(),
                    slot: b.slot.to_string(),
                    domain: b.domain.map(|d| d.to_string()),
                    scope: crate::dto::mult_scope_tag(b.scope).to_string(),
                    worked: log.worked_values(b.slot),
                    club: self.fd_club_worked(b.slot),
                })
                .collect(),
            // What the journal restore kept out of this session, and why.
            kept_out: match log.held() {
                (0, 0) => None,
                (contest, running) => Some(crate::dto::FdKeptOutDto {
                    other_contest: contest as u32,
                    other_running: running as u32,
                }),
            },
            // The journal's writes that failed since its last good one: the operator is told.
            unsaved: self
                .station
                .fd_log_path
                .as_ref()
                .and_then(|p| self.station.journals.trouble(p))
                .map(|t| crate::dto::FdUnsavedDto {
                    failed_writes: t.failed,
                    last_saved_unix: t.last_good_unix,
                }),
            operator_now: log.operator.clone(),
            operators: log
                .operator_stats(now_unix_secs())
                .into_iter()
                .map(crate::dto::FdOperatorDto::from)
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
        // ⭐ The club board's rows as the capture carries them, and the only ones it ships (cut to
        // them below): on a host the board its club sends every position now, never its own list of
        // every position, which can be longer than the hosted page takes; on a position the board
        // the host sent it (`Engine::fd_club_board_sent`).
        let board = self.fd_club_board_sent(crate::engine::now_unix_secs());
        // ROWS, which is what the capture carries and the browser bounds: `qso_count` leaves out a
        // logged dupe and a satellite contact the contest gives no credit, so it can sit under the
        // bound while the log it ships is over it.
        if log.qsos().len() > 2048
            || (self.fd_sync_enabled()
                && (self.fd_mirror.dupes.len() > 4096
                    // The generalised keys now ship too, and they are the SAME contacts
                    // under a wider key — so they get the same count bound. Unbounded,
                    // a host could grow the capture past the limit the triple enforces.
                    || self.fd_mirror.dkeys.len() > 4096
                    || board.len() > 128
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
        // …and the one string rendered from them, which is bounded by the browser's own
        // per-string rule and must be refused here first rather than there.
        check(&self.contest_sent_exchange().unwrap_or_default())?;
        check(&self.contest_composing_text().unwrap_or_default())?;
        check(&self.contest_composing_text_phone().unwrap_or_default())?;
        // The advisory band list comes out of a rules file, which bounds nothing about it.
        for b in log.ruleset().bands {
            check(b)?;
        }
        // …and the location warning echoes what the operator typed.
        if let Some(w) = &log.session.location_warning {
            check(&w.typed)?;
            for h in &w.hints {
                check(h)?;
            }
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
            // The received values a contest that is not Field Day carries per row
            // (`FieldDayQso::rcvd`). Field Day's are the class/section just checked, and
            // are not serialised twice, so they are not counted twice either.
            if log.session.exchange.name != "fieldday" {
                for v in &q.rx {
                    check(&v.raw)?;
                }
            }
        }
        // The per-operator block names each operator once, and the bands and modes they worked.
        for o in log.operator_stats(crate::engine::now_unix_secs()) {
            check(&o.operator)?;
            for (band, mode, _) in &o.band_mode {
                check(band)?;
                check(mode)?;
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
            // Same treatment for the generalised keys: they are host-supplied strings
            // reaching a Remote capture, so they are measured, not trusted.
            for k in &self.fd_mirror.dkeys {
                for s in k {
                    check(s)?;
                }
            }
            for r in &board {
                for s in [&r.pos, &r.name, &r.band, &r.mode, &r.op] {
                    check(s)?;
                }
            }
            for s in &self.fd_mirror.sections {
                check(s)?;
            }
        }
        let mut status = self.field_day_status();
        keep_the_board_sent(&mut status, &board);
        Ok(status)
    }

    /// ⭐ **The values the CLUB has worked in multiplier slot `slot`**, sorted — what every
    /// laptop's board lights, whichever position worked it: on the host straight from its club
    /// log, the list its positions are sent ([`crate::fdevent::ClubLog::worked_mults`]); on a
    /// position, the list its host sent (`ClubMirror::mults`), empty from a host older than it.
    /// Empty without club sync: a laptop on its own has only its own board.
    fn fd_club_worked(&self, slot: &str) -> Vec<String> {
        if !self.fd_sync_enabled() {
            return Vec::new();
        }
        match &self.fd_club {
            Some(club) => club.worked_mults(slot),
            None => {
                let mut values: Vec<String> = self
                    .fd_mirror
                    .mults
                    .iter()
                    .filter(|(s, _)| s == slot)
                    .map(|(_, v)| v.clone())
                    .collect();
                values.sort();
                values
            }
        }
    }

    /// ⭐ **What Remote's snapshot carries of this laptop's club board**: the board every position
    /// is sent ([`Engine::fd_club_board_sent`]), as the Remote capture carries it, never the host's
    /// own list of every position its club knows, which grows with each position that ever joined.
    /// `field_day` is that snapshot's Field Day block.
    pub fn fd_club_board_to_remote(&self, field_day: &mut Option<FieldDayStatus>) {
        keep_the_board_sent(field_day, &self.fd_club_board_sent(now_unix_secs()));
    }
}

/// Keep, of a Field Day block's club board, the lines of the positions in `sent` and no others:
/// the host's own board lists every position its club knows, and what leaves the host carries the
/// board every position is sent. A position's board is those rows already.
fn keep_the_board_sent(
    field_day: &mut Option<FieldDayStatus>,
    sent: &[tempo_net::fdsync::WireBoardRow],
) {
    if let Some(club) = field_day.as_mut().and_then(|f| f.club.as_mut()) {
        let sent: std::collections::HashSet<&str> = sent.iter().map(|r| r.pos.as_str()).collect();
        club.board.retain(|r| sent.contains(r.posid.as_str()));
    }
}

/// One contest row as the log table shows it — the snapshot's `log`, and the contest screen's
/// Removed list, which must render a removed contact exactly as the row it was.
pub(super) fn field_day_qso(
    q: &tempo_core::fieldday::LoggedQso,
    rs: &tempo_core::fd_rules::FdRuleset,
    spec: &'static tempo_core::contest::ExchangeSpec,
    role: &'static tempo_core::contest::RoleSpec,
    nr: Option<u32>,
) -> FieldDayQso {
    FieldDayQso {
        call: q.call.clone(),
        class: q.class().to_string(),
        section: q.section().to_string(),
        band: q.band.clone(),
        mode: q.mode.clone(),
        submode: q.submode.clone(),
        when_unix: q.when_unix,
        // The bird, for the while-typing verdict — see `FieldDayQso::sat`.
        sat: q.sat.as_ref().map(|s| s.bird.clone()).unwrap_or_default(),
        // ⭐ THE ROW'S OWN SENT EXCHANGE (§3.3). Rendered from the
        // row by the one function that renders one, so an emitter
        // looping over these rows has the right value in hand and
        // no reason to reach out to the session.
        mex: tempo_core::contest::sent_exchange_string(q, spec),
        // The same, slot by slot, for the one emitter that fills boxes rather than a line.
        sent: tempo_core::contest::sent_exchange(q, spec)
            .into_iter()
            .map(|v| (v.key.to_string(), v.raw))
            .collect(),
        // What this contact RECEIVED, aligned with `receives` below — the
        // contest log table's columns. Field Day's already ride class/section.
        rcvd: if spec.name == "fieldday" {
            Vec::new()
        } else {
            role.receives
                .iter()
                .map(|k| q.rcvd(k).to_string())
                .collect()
        },
        // ⭐ The row's key under THIS ruleset's rule, from the one builder.
        // The strip cannot build it: the rule names sent slots, and a row's
        // sent exchange reaches the UI only as the rendered `mex`.
        // A satellite contact the contest gives no credit is a dupe of
        // nothing, so it carries no key the verdict could match.
        dkey: if rs.satellite_credit || q.sat.is_none() {
            rs.dupe_rule.key(q)
        } else {
            Vec::new()
        },
        // ⭐ The engine's own answer about this row, so the log table marks a
        // zero-scoring dupe instead of guessing at one from a repeated callsign.
        dupe: q.dupe,
        // …and whether it was worked in a mode the contest bans, which counts for nothing.
        banned: q.banned_under(rs),
        seq: q.seq,
        nr,
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

    /// ⭐ **A laptop out of the club shows its own board, whatever its mirror still holds** — the
    /// club's list is read only while club sync runs, and the mirror keeps the last host's state
    /// until a next host's snapshot replaces it. CONTROL: joined, the same mirror lights the
    /// club's, and this laptop's own stays its own.
    #[test]
    fn a_laptop_out_of_the_club_shows_its_own_board() {
        let mut e = engine("arrlfd", 2);
        e.fd_mirror_mut().apply(&tempo_net::fdsync::ClubState {
            reset: true,
            mults: vec![
                ("SECTION".into(), "EMA".into()),
                ("SECTION".into(), "MN".into()),
            ],
            ..Default::default()
        });
        let board = |e: &Engine| e.field_day_status().unwrap().boards[0].clone();
        assert_eq!(board(&e).worked, ["EMA", "WI"]);
        assert!(board(&e).club.is_empty(), "no club sync, no club list");
        let mut s = e.settings().clone();
        s.fd_join_addr = "192.168.1.10:42073".into();
        s.fd_position_id = "eeee0001".into();
        e.apply_settings(s);
        assert_eq!(board(&e).club, ["EMA", "MN"], "CONTROL: joined");
        assert_eq!(board(&e).worked, ["EMA", "WI"]);
    }

    /// ⭐ **A position's board names the position its host removed**, from the host's own line
    /// (`WireBoardRow::removed`), with the one link word a position's board carries; every other
    /// line carries none, as before. Remote's capture leaves the word out with the host's own
    /// (`leave_out_the_hosts_own`).
    #[test]
    fn a_positions_board_names_the_position_its_host_removed() {
        let mut e = engine("arrlfd", 0);
        let mut s = e.settings().clone();
        s.fd_join_addr = "192.168.1.10:42073".into();
        s.fd_position_id = "eeee0001".into();
        e.apply_settings(s);
        let row = |pos: &str, removed: bool| tempo_net::fdsync::WireBoardRow {
            pos: pos.into(),
            name: pos.into(),
            qsos: 3,
            age: 2,
            removed,
            ..Default::default()
        };
        e.fd_mirror_mut().apply(&tempo_net::fdsync::ClubState {
            reset: true,
            board: vec![row("bbbb0002", false), row("cccc0003", true)],
            ..Default::default()
        });
        let board = e.field_day_status().unwrap().club.unwrap().board;
        let links: Vec<(String, Option<String>)> =
            board.into_iter().map(|r| (r.posid, r.link)).collect();
        assert_eq!(
            links,
            [
                ("bbbb0002".to_string(), None),
                ("cccc0003".to_string(), Some("removed".to_string()))
            ]
        );
    }

    /// ⭐ **The status names who is at the key and carries each operator's figures**, with the
    /// rates as contacts per hour: one contact in the last ten minutes is 6 an hour.
    #[test]
    fn the_status_carries_each_operators_figures_and_who_is_at_the_key() {
        let mut e = engine("arrlfd", 0);
        let now = now_unix_secs();
        let Mode::FieldDay { station, .. } = &mut e.mode else {
            panic!("active")
        };
        station.log.operator = "K9AAA".into();
        assert!(station
            .log
            .log_mode_at("K1A", "2A", "WI", "CW", 0, now - 60));
        assert!(station
            .log
            .log_mode_at("K1B", "2A", "IL", "PH", 0, now - 1200));
        station.log.operator = "N9BBB".into();
        assert!(station
            .log
            .log_mode_at("K1C", "2A", "OH", "PH", 0, now - 4000));
        let st = e.field_day_status().expect("a contest is on");
        assert_eq!(st.operator_now, "N9BBB");
        let ops: Vec<(String, u32, u32, u32, u32, u32)> = st
            .operators
            .iter()
            .map(|o| {
                (
                    o.operator.clone(),
                    o.qsos,
                    o.points,
                    o.new_mults,
                    o.rate10,
                    o.rate60,
                )
            })
            .collect();
        assert_eq!(
            ops,
            vec![
                ("K9AAA".into(), 2, 3, 0, 6, 2),
                ("N9BBB".into(), 1, 1, 0, 0, 0)
            ]
        );
        assert_eq!(
            st.operators[0]
                .band_mode
                .iter()
                .map(|c| c.qsos)
                .sum::<u32>(),
            2
        );
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

    /// ⭐ **A host's capture carries the board its bound measured**: the board its club sends every
    /// position now, cut to what one club line carries, so a host whose log has heard of more
    /// positions than the hosted page takes (128) still ships a capture the page reads, while the
    /// host's own board lists every one of them; its own loopback position need not be in for the
    /// host to see who is. Each row says where its link stands, from the live club log. CONTROL:
    /// the club log does know every one of them.
    #[test]
    fn a_hosts_capture_carries_the_board_its_bound_measured() {
        let dir = std::env::temp_dir().join(format!("fdd-host-bound-{}", std::process::id()));
        let mut e = engine("arrlfd", 0);
        e.settings.fd_host_enable = true;
        e.settings.fd_position_id = "aaaa0001".into();
        e.fd_host_start(dir.join("fd_event_bound.jsonl")).unwrap();
        let now = crate::engine::now_unix_secs();
        let club = e.fd_club.as_mut().unwrap();
        for i in 0..130u32 {
            let id = format!("{:08x}", 0xb000_0000u32 + i);
            club.join(&id, "", "W1AW", now);
            club.open_link(&id);
        }
        // What the club sends each position: the positions heard from most recently, as many as
        // one club line carries.
        let sent: std::collections::BTreeSet<String> =
            tempo_net::fdsync::board_fit(&club.board_rows(now))
                .rows
                .into_iter()
                .map(|r| r.pos)
                .collect();
        assert_eq!(
            e.fd_club.as_ref().unwrap().positions().len(),
            130,
            "CONTROL"
        );
        assert!(sent.len() < 130, "premise: one club line cannot carry 130");
        assert!(
            e.fd_mirror.board.is_empty(),
            "premise: the host's own position is not in"
        );
        let value = e.bounded_field_day_status().unwrap().unwrap();
        let board = value.club.unwrap().board;
        assert!(
            board.len() <= 128,
            "the hosted page takes at most 128: {}",
            board.len()
        );
        assert_eq!(
            board
                .iter()
                .map(|r| r.posid.clone())
                .collect::<std::collections::BTreeSet<_>>(),
            sent,
            "the board the club sends"
        );
        assert!(
            board.iter().all(|r| r.link.as_deref() == Some("connected")),
            "each row's link, from the live club log: {board:?}"
        );
        assert_eq!(
            e.snapshot()
                .field_day
                .unwrap()
                .club
                .unwrap()
                .board
                .into_iter()
                .map(|r| r.posid)
                .collect::<std::collections::BTreeSet<_>>(),
            e.fd_club
                .as_ref()
                .unwrap()
                .positions()
                .keys()
                .cloned()
                .collect(),
            "the host's own board lists every one of them"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The host's own position in [`a_full_club`], and the tent in it that went quiet.
    const HQ: &str = "aaaa0001";
    const QUIET: &str = "dddd0004";

    /// ⭐ **A host whose club has more positions than one club line carries**: its own position,
    /// 69 tents connected and heard from now, and the Digital tent, which joined and was last heard
    /// ten minutes ago, so the board every position is sent leaves it out first. Every stamp is
    /// `t0` or ten minutes before it, so the board the club sends at `t0` is the same bytes on
    /// every run. The engine, `t0`, and every position id the club knows.
    fn a_full_club(dir: &std::path::Path) -> (Engine, u64, std::collections::BTreeSet<String>) {
        let mut e = engine("arrlfd", 0);
        e.settings.fd_host_enable = true;
        e.settings.fd_position_id = HQ.into();
        e.fd_host_start(dir.join("fd_event_full.jsonl")).unwrap();
        let t0 = crate::engine::now_unix_secs();
        let club = e.fd_club.as_mut().unwrap();
        let mut tent = |id: &str, name: &str, at: u64| {
            club.join(id, name, "K1ABC", at);
            club.position_status(
                id,
                &tempo_net::fdsync::PosReport {
                    band: "20m".into(),
                    mode: "CW".into(),
                    op: "K1ABC".into(),
                    freq: 14_025_000,
                    name: name.into(),
                    clock_ms: None,
                },
                at,
            );
            id.to_string()
        };
        let mut ids = std::collections::BTreeSet::new();
        ids.insert(tent(HQ, "HQ", t0));
        for i in 1..=69u32 {
            let id = format!("{:08x}", 0xc000_0000u32 + i);
            ids.insert(tent(
                &id,
                &format!("Position {i:02} tent, the north field"),
                t0,
            ));
        }
        ids.insert(tent(QUIET, "Digital tent, by the tower", t0 - 600));
        // A laptop is connected as each of them but the quiet one: the host's own over loopback.
        for id in ids.iter().filter(|id| *id != QUIET) {
            club.open_link(id);
        }
        (e, t0, ids)
    }

    /// The board every position is sent at host clock `now`: the rows the first club line carries,
    /// cut from the club's board by `fdsync::board_fit` as the line writer cuts it.
    fn wire_board(e: &Engine, now: u64) -> Vec<tempo_net::fdsync::WireBoardRow> {
        let club = e.fd_club.as_ref().unwrap();
        tempo_net::fdsync::board_fit(&club.club_state(0, 0, 0, now).board).rows
    }

    /// ⭐ **A host lists every joined position on its own board, however many, and removes one the
    /// board every position is sent leaves out.** One club line carries about 59 positions, so the
    /// board every position is sent is cut to the ones heard from most recently; the host's own
    /// board, where each line has its Status and its Remove, is not, so no position can be kept off
    /// the host's screen by filling the club with others or by going quiet. The host's count of
    /// connected positions counts the Connected lines of that same board, beside it and on its own.
    /// Read by value off the snapshot the host's screen reads; Remove is the one its button calls,
    /// with the row's id, and the removed position's next JOIN is turned away.
    #[test]
    fn a_host_lists_every_joined_position_and_removes_one_the_board_it_sends_leaves_out() {
        let dir = std::env::temp_dir().join(format!("fdd-host-every-{}", std::process::id()));
        let (mut e, t0, ids) = a_full_club(&dir);
        let sent: std::collections::BTreeSet<String> =
            wire_board(&e, t0).into_iter().map(|r| r.pos).collect();
        assert!(
            sent.len() < 70 && !sent.contains(QUIET),
            "premise: the board every position is sent leaves the quiet tent out: {} of {}",
            sent.len(),
            ids.len()
        );
        let connected = |board: &[crate::dto::FdClubBoardRow]| {
            board
                .iter()
                .filter(|r| r.link.as_deref() == Some("connected"))
                .count()
        };
        let s = e.snapshot();
        let board = s.field_day.unwrap().club.unwrap().board;
        assert_eq!(
            board
                .iter()
                .map(|r| r.posid.clone())
                .collect::<std::collections::BTreeSet<_>>(),
            ids,
            "the host's own board lists every position its club knows"
        );
        let quiet = board.iter().find(|r| r.posid == QUIET).unwrap();
        assert_eq!(quiet.pos_name, "Digital tent, by the tower");
        assert_eq!(quiet.link.as_deref(), Some("lost"));
        assert!(quiet.heard_secs.is_some_and(|h| h >= 600), "{quiet:?}");
        assert_eq!(connected(&board), 69, "every tent but the quiet one");
        assert_eq!(
            s.club_link.unwrap().hosting.unwrap().connected,
            69,
            "the hosting line beside the board"
        );
        assert_eq!(
            e.fd_club_link().unwrap().hosting.unwrap().connected,
            69,
            "the hosting line on its own, as Settings reads it with no board beside it"
        );
        // Remove, as the button on its line presses it.
        assert_eq!(e.fd_club_remove_position(&quiet.posid), Ok(()));
        let s = e.snapshot();
        let board = s.field_day.unwrap().club.unwrap().board;
        assert_eq!(board.len(), ids.len(), "a removed position keeps its line");
        assert_eq!(
            board
                .iter()
                .find(|r| r.posid == QUIET)
                .and_then(|r| r.link.as_deref()),
            Some("removed"),
            "its line says so, beside Allow back"
        );
        assert_eq!(connected(&board), 69);
        assert_eq!(s.club_link.unwrap().hosting.unwrap().connected, 69);
        // Its next try is turned away, by the removal's own sentence.
        let v = tempo_net::fdsync::PROTO_VERSION;
        let hash = "c".repeat(64);
        let join = |e: &mut Engine, id: &str, name: &str| {
            e.fd_club_join(v, id, name, "K1ABC", "arrlfd", "", &hash)
                .map(|_| ())
        };
        assert_eq!(
            join(&mut e, QUIET, "Digital tent, by the tower"),
            Err(crate::fdevent::POSITION_REMOVED.to_string())
        );
        assert_eq!(
            join(&mut e, "eeee0005", "Satellite tent"),
            Ok(()),
            "CONTROL: a position the host did not remove is served"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⭐ CONTROL: **the board every position is sent is the bytes it was** for the same club: cut
    /// to what one club line carries, the quiet tent left out first, and within that line; byte for
    /// byte the board this club sent before the host's own board listed every position (club sync
    /// stays v4), pinned by its SHA-256 as the earlier build wrote it; and what the club's
    /// connections are sent (`Engine::fd_club_state`, each heartbeat's) is that board.
    #[test]
    fn the_board_every_position_is_sent_stays_the_same_bytes() {
        let dir = std::env::temp_dir().join(format!("fdd-host-wire-{}", std::process::id()));
        let (mut e, t0, ids) = a_full_club(&dir);
        let wire = wire_board(&e, t0);
        assert!(
            wire.len() < ids.len() && wire.iter().all(|r| r.pos != QUIET),
            "cut: {} of {}",
            wire.len(),
            ids.len()
        );
        let line = tempo_net::fdsync::encode_line(&tempo_net::fdsync::Msg::Club(
            tempo_net::fdsync::ClubState {
                score: u32::MAX,
                qsos: u64::MAX,
                board: wire.clone(),
                ..Default::default()
            },
        ));
        assert!(
            line.len() <= tempo_net::fdsync::MAX_LINE_BYTES,
            "the longest club line it rides: {}",
            line.len()
        );
        assert_eq!(
            crate::fdevent::sha256_hex(&serde_json::to_string(&wire).unwrap()),
            "bebc6aacc2a77986008802943dc5aee621e5121634ecbb829220bde0fd7d5270"
        );
        let now_sent: std::collections::BTreeSet<String> =
            tempo_net::fdsync::board_fit(&e.fd_club_state(0, 0, "").board)
                .rows
                .into_iter()
                .map(|r| r.pos)
                .collect();
        assert_eq!(
            now_sent,
            wire.into_iter().map(|r| r.pos).collect(),
            "what each heartbeat sends"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⭐ **The Remote capture carries the board every position is sent, never the host's own
    /// list**: its rows are the cut every position gets, string for string the ones its bound
    /// measures, within the 128 the hosted page takes, the quiet tent left out as each position's
    /// board leaves it out. CONTROL: the host's own snapshot beside it lists every position.
    #[test]
    fn a_hosts_remote_capture_carries_the_board_every_position_is_sent() {
        let dir = std::env::temp_dir().join(format!("fdd-host-remote-{}", std::process::id()));
        let (e, _, ids) = a_full_club(&dir);
        let strings = |rows: Vec<[String; 5]>| {
            rows.into_iter()
                .map(|r| (r[0].clone(), r))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let sent = strings(
            tempo_net::fdsync::board_fit(
                &e.fd_club
                    .as_ref()
                    .unwrap()
                    .board_rows(crate::engine::now_unix_secs()),
            )
            .rows
            .into_iter()
            .map(|r| [r.pos, r.name, r.band, r.mode, r.op])
            .collect(),
        );
        let shipped = e.bounded_field_day_status().unwrap().unwrap();
        let shipped = shipped.club.unwrap().board;
        assert!(shipped.len() <= 128, "{}", shipped.len());
        let shipped = strings(
            shipped
                .into_iter()
                .map(|r| [r.posid, r.pos_name, r.band, r.mode, r.operator])
                .collect(),
        );
        assert!(!shipped.contains_key(QUIET));
        assert_eq!(shipped, sent, "the board every position is sent");
        assert_eq!(
            e.snapshot()
                .field_day
                .unwrap()
                .club
                .unwrap()
                .board
                .into_iter()
                .map(|r| r.posid)
                .collect::<std::collections::BTreeSet<_>>(),
            ids,
            "CONTROL: the host's own board"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
                removed: false,
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
    /// ⭐ **A row of a contest that is not Field Day carries what it RECEIVED**, one value
    /// per slot the session receives, in `receives` order — the contest log table's
    /// columns. A DTO row carried `class` and `section` only, so every CQ WW or QSO-party
    /// row showed two empty cells and nothing it had actually copied.
    #[test]
    fn a_contest_row_carries_its_received_values_and_a_field_day_row_does_not() {
        let mut e = Engine::with_settings(crate::settings::Settings {
            mycall: "W8ABC".into(),
            fd_active: true,
            fd_event: "ohqp".into(),
            contest_qth_state: "MI".into(),
            ..Default::default()
        });
        e.restore_field_day_if_enabled();
        let ex = vec![
            ("RST".to_string(), "579".to_string()),
            ("QTH".to_string(), "CUYA".to_string()),
        ];
        assert!(e.contest_log_manual("W8XYZ", &ex, "CW", None).unwrap());
        let fd = e.snapshot().field_day.expect("in the party");
        assert_eq!(
            fd.receives
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>(),
            vec!["RST", "QTH"]
        );
        assert_eq!(fd.log[0].rcvd, vec!["579".to_string(), "CUYA".to_string()]);

        // CONTROL: Field Day's row is unchanged on the wire — its two received values
        // already ride class/section, and no `rcvd` key is sent at all.
        let e = engine("arrlfd", 1);
        let fd = e.snapshot().field_day.expect("in Field Day");
        assert!(fd.log[0].rcvd.is_empty());
        let wire = serde_json::to_value(&fd.log[0]).unwrap();
        assert!(wire.get("rcvd").is_none(), "{wire}");
        assert_eq!(wire["class"], "2A");
    }

    /// ⭐ **The while-typing verdict must ask the ENGINE's question.** The strip built a
    /// hardcoded `(call, band, mode class)` triple, which is the rule for exactly two of
    /// the eighteen shipped rulesets. Sweepstakes keys on the CALL ALONE (rule 2.2 —
    /// `by_band: false`), so the strip said "new" for a station the log then refused; a
    /// QSO party keys on the counties too, so it said DUPE for a legal contact with a
    /// mobile in a new county. `contest::dupe` names that second direction the costly
    /// one: *"over-reporting refuses a legal contact"*.
    ///
    /// So the rule and each row's key ride the snapshot, and the strip compares keys
    /// instead of rebuilding one. Three rulesets here, each with a different key WIDTH,
    /// because a constant or empty implementation passes any single one of them.
    #[test]
    fn a_contest_row_carries_the_rulesets_own_dupe_key_and_the_rule_that_built_it() {
        let mut e = Engine::with_settings(crate::settings::Settings {
            mycall: "W8ABC".into(),
            fd_active: true,
            fd_event: "ohqp".into(),
            contest_qth_state: "MI".into(),
            ..Default::default()
        });
        e.restore_field_day_if_enabled();
        let ex = vec![
            ("RST".to_string(), "579".to_string()),
            ("QTH".to_string(), "CUYA".to_string()),
        ];
        assert!(e.contest_log_manual("W8XYZ", &ex, "CW", None).unwrap());
        let wire = serde_json::to_value(e.snapshot().field_day.expect("in the party")).unwrap();
        // The rule as the ruleset declares it — the county counts, in BOTH directions
        // (working someone else's mobile, and being one).
        assert_eq!(wire["dupeRule"]["byCall"], true, "{}", wire["dupeRule"]);
        assert_eq!(wire["dupeRule"]["byBand"], true, "{}", wire["dupeRule"]);
        assert_eq!(
            wire["dupeRule"]["byModeClass"], true,
            "{}",
            wire["dupeRule"]
        );
        assert_eq!(wire["dupeRule"]["byFields"], serde_json::json!(["QTH"]));
        assert_eq!(wire["dupeRule"]["bySentFields"], serde_json::json!(["QTH"]));
        // …and the row's own key is what that rule builds: five components, the received
        // county among them. Width and content, so a truncated key cannot pass.
        let key = wire["log"][0]["dkey"]
            .as_array()
            .expect("a row key")
            .clone();
        assert_eq!(key.len(), 5, "{key:?}");
        assert_eq!(key[0], "W8XYZ");
        assert_eq!(key[3], "CUYA", "the RECEIVED county is in the key: {key:?}");

        // CONTROL 1 — Field Day, the rule the old triple was written for: three
        // components and no named slots. This is the case that used to be right.
        let wire = serde_json::to_value(engine("arrlfd", 1).snapshot().field_day.unwrap()).unwrap();
        assert_eq!(wire["dupeRule"]["byFields"], serde_json::json!([]));
        assert_eq!(wire["dupeRule"]["bySentFields"], serde_json::json!([]));
        assert_eq!(wire["log"][0]["dkey"].as_array().unwrap().len(), 3);
        // …and Field Day LOGS a dupe, marked, as every contest does since the operator's ruling
        // of 2026-10-11, which is what the card's wording turns on.
        assert_eq!(wire["dupeRule"]["logDupes"], true);

        // CONTROL 2 — Sweepstakes, the other direction: the band and the mode class are
        // NOT in the key, so a station worked on 40m CW is a dupe on 20m SSB.
        let mut e = Engine::with_settings(crate::settings::Settings {
            mycall: "W8ABC".into(),
            fd_active: true,
            fd_event: "arrlss_cw".into(),
            // Sweepstakes sends check + section from settings, and derives its
            // PRECEDENCE from the declared categories. Without all four the session
            // never starts and the control would pass vacuously.
            contest_check: "68".into(),
            fd_section: "EMA".into(),
            contest_category_assisted: "NON-ASSISTED".into(),
            contest_category_power: "LOW".into(),
            ..Default::default()
        });
        e.restore_field_day_if_enabled();
        let wire = serde_json::to_value(e.snapshot().field_day.expect("in SS")).unwrap();
        assert_eq!(wire["dupeRule"]["byCall"], true);
        assert_eq!(wire["dupeRule"]["byBand"], false, "SS rule 2.2");
        assert_eq!(wire["dupeRule"]["byModeClass"], false, "SS rule 2.2");
        // ⭐ …and Sweepstakes KEEPS a dupe, scored zero. The card must not tell this operator
        // the contact "will be refused": CQ and ARRL both ask entrants NOT to drop the row,
        // because a contact missing from your log costs the other station its credit.
        assert_eq!(wire["dupeRule"]["logDupes"], true, "SS cross-checks");
    }

    /// ⭐ **The club half was truncated at the last hop.** `fdsync` has shipped the
    /// generalised `dkeys` beside the legacy `dupes` triple since the wire was
    /// generalised — `dupes` is documented there as *"this list projected back to its
    /// first three components"* — and `ClubMirror` holds both. Only the triple reached
    /// the UI, so the club warning asked the Field Day question in every contest.
    #[test]
    fn the_club_block_carries_the_generalised_dupe_keys_not_only_the_legacy_triple() {
        let mut e = engine("arrlfd", 1);
        e.settings.fd_join_addr = "127.0.0.1:7878".into();
        e.fd_mirror.connected = true;
        // The own row's key READ FROM THE SNAPSHOT, never guessed: its band comes from
        // the live rig, and a hardcoded one would make the subtraction below vacuous.
        let own_key = e.snapshot().field_day.expect("in FD").log[0].dkey.clone();
        assert!(!own_key.is_empty(), "a row key to subtract");
        let club_only = vec!["K9CLUB".to_string(), "20M".to_string(), "CW".to_string()];
        e.fd_mirror.dkeys = [own_key, club_only.clone()].into_iter().collect();
        e.fd_mirror.dupes = [("K1ABC".to_string(), "20m".to_string(), "CW".to_string())]
            .into_iter()
            .collect();
        let wire = serde_json::to_value(e.snapshot().field_day.expect("in FD")).unwrap();
        // BOTH DIRECTIONS in one assertion: the club-only key reaches the UI (the whole
        // point), and the key the own log already carries is subtracted — club-only,
        // exactly as the legacy `dupes` beside it. A filter that emptied the list, or
        // one that subtracted nothing, fails on one side or the other.
        assert_eq!(
            wire["club"]["dkeys"],
            serde_json::json!([club_only]),
            "{}",
            wire["club"]
        );
        // CONTROL — the legacy triple still ships unchanged beside it, so a reader of
        // either is served and the two cannot silently swap.
        assert_eq!(
            wire["club"]["dupes"],
            serde_json::json!([["K1ABC", "20m", "CW"]])
        );
    }

    /// ⭐ **A LOGGED DUPE MUST SAY SO ON THE WIRE.** In the ten cross-checked contests a
    /// duplicate is now logged and scored zero rather than refused, so the contest log table
    /// gained rows that score nothing. Without a flag on the row the operator reviewing their
    /// log after the event cannot tell a zero-scoring dupe from a real contact, cannot check
    /// the sponsor's math against their own, and cannot tell an intended dupe from a logging
    /// mistake — an ambiguity that did not exist while a dupe was never logged at all.
    ///
    /// The UI must not re-derive this. It had been doing exactly that, over a fourth copy of
    /// the key, which is what the row flag replaces.
    #[test]
    fn a_logged_dupe_says_so_on_the_row_in_every_contest() {
        let mut e = Engine::with_settings(crate::settings::Settings {
            mycall: "W1ABC".into(),
            fd_active: true,
            fd_event: "arrlss_cw".into(),
            contest_check: "68".into(),
            fd_section: "EMA".into(),
            contest_category_assisted: "NON-ASSISTED".into(),
            contest_category_power: "LOW".into(),
            ..Default::default()
        });
        e.restore_field_day_if_enabled();
        let ex = |nr: &str| {
            vec![
                ("NR".to_string(), nr.to_string()),
                ("PREC".to_string(), "A".to_string()),
                ("CALL".to_string(), "K9XYZ".to_string()),
                ("CK".to_string(), "72".to_string()),
                ("SEC".to_string(), "IL".to_string()),
            ]
        };
        // The same station twice. Sweepstakes works a station ONCE — neither band nor mode
        // class is in its key — so the second is a dupe by the ruleset's own rule, and its
        // `log_dupes` says the sponsor wants it in the log rather than refused.
        assert!(e.contest_log_manual("K9XYZ", &ex("1"), "CW", None).unwrap());
        assert!(e.contest_log_manual("K9XYZ", &ex("2"), "CW", None).unwrap());
        let wire = serde_json::to_value(e.snapshot().field_day.expect("in SS")).unwrap();
        assert_eq!(
            wire["log"].as_array().unwrap().len(),
            2,
            "the dupe is LOGGED"
        );
        assert_eq!(wire["log"][1]["dupe"], true, "{}", wire["log"][1]);
        // The ordinary row says nothing rather than saying false — the flag is skipped when
        // false so it does not spend a Remote capture's byte bound on every real contact.
        assert!(
            wire["log"][0].get("dupe").is_none(),
            "the first contact is real: {}",
            wire["log"][0]
        );

        // …and Field Day, which logs its dupes too since the operator's ruling of 2026-10-11:
        // the repeat is a row, and the row says so.
        let mut e = engine("arrlfd", 1);
        let Mode::FieldDay { station, .. } = &mut e.mode else {
            panic!("active")
        };
        let before = station.log.qsos().len();
        assert!(station.log.log_mode_at("K1T0", "2A", "WI", "PH", 99, 1000));
        let wire = serde_json::to_value(e.snapshot().field_day.unwrap()).unwrap();
        assert_eq!(wire["log"].as_array().unwrap().len(), before + 1);
        assert_eq!(wire["log"][before]["dupe"], true, "{}", wire["log"][before]);
        assert!(wire["log"][0].get("dupe").is_none(), "{}", wire["log"][0]);
    }

    /// ⭐ **A contact in a mode the contest bans says so on the row, and the count leaves it out.**
    /// The Illinois QSO Party gives FT8 *"no contact credit"*, so the contact is logged and worth
    /// nothing, and the hosted Remote page, which recounts `qsoCount` from the rows and refuses a
    /// capture whose count it cannot reproduce, has to be able to tell it from a contact that
    /// counts. Skipped when false, like `dupe`.
    /// ⭐ **Every row carries its QSO number on the wire**, in a contest that sends no serial as
    /// in one that does — the number the log table shows (`FieldDayLog::qso_numbers`).
    #[test]
    fn every_contest_row_carries_its_qso_number() {
        let mut e = Engine::with_settings(crate::settings::Settings {
            mycall: "W9XYZ".into(),
            fd_active: true,
            fd_event: "ilqp".into(),
            contest_qth_state: "IL".into(),
            contest_qth_county: "MCLN".into(),
            ..Default::default()
        });
        e.restore_field_day_if_enabled();
        for (call, qth) in [("K9AAA", "COOK"), ("W9BBB", "LAKE"), ("K9AAA", "COOK")] {
            let ex = vec![
                ("RST".to_string(), "599".to_string()),
                ("QTH".to_string(), qth.to_string()),
            ];
            assert!(e.contest_log_manual(call, &ex, "CW", None).unwrap());
        }
        let fd = e.snapshot().field_day.expect("in the party");
        assert_eq!(
            fd.log.iter().map(|q| q.nr).collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3)],
            "the dupe is numbered too"
        );
        let wire = serde_json::to_value(&fd).unwrap();
        assert_eq!(wire["log"][2]["nr"], 3, "{}", wire["log"][2]);
    }

    #[test]
    fn a_banned_mode_contact_says_so_on_the_row_and_counts_for_nothing() {
        let mut e = Engine::with_settings(crate::settings::Settings {
            mycall: "W9XYZ".into(),
            fd_active: true,
            fd_event: "ilqp".into(),
            contest_qth_state: "IL".into(),
            contest_qth_county: "MCLN".into(),
            ..Default::default()
        });
        e.restore_field_day_if_enabled();
        let ex = |qth: &str| {
            vec![
                ("RST".to_string(), "599".to_string()),
                ("QTH".to_string(), qth.to_string()),
            ]
        };
        assert!(e
            .contest_log_manual("K9AAA", &ex("COOK"), "CW", None)
            .unwrap());
        assert!(e
            .contest_log_manual("K9FT8", &ex("LAKE"), "DIG", Some("FT8"))
            .unwrap());
        let fd = e.snapshot().field_day.expect("in the party");
        let county: Vec<&str> = fd
            .boards
            .iter()
            .find(|b| b.domain.as_deref() == Some("il_counties"))
            .expect("the county board")
            .worked
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(
            (fd.log.len(), fd.qso_count, fd.total_score, county),
            (2, 1, 2, vec!["COOK"]),
            "(rows, contacts, score, the county board): the FT8 contact is logged and worth nothing"
        );
        let wire = serde_json::to_value(&fd).unwrap();
        assert_eq!(wire["log"][1]["banned"], true, "{}", wire["log"][1]);
        assert!(
            wire["log"][0].get("banned").is_none(),
            "the CW contact says nothing: {}",
            wire["log"][0]
        );
        // CONTROL: the same contact on RTTY counts, and its row carries no mark.
        assert!(e
            .contest_log_manual("K9RTY", &ex("LAKE"), "DIG", Some("RTTY"))
            .unwrap());
        let fd = e.snapshot().field_day.expect("in the party");
        assert_eq!((fd.log.len(), fd.qso_count), (3, 2));
        let wire = serde_json::to_value(&fd).unwrap();
        assert!(wire["log"][2].get("banned").is_none(), "{}", wire["log"][2]);
    }

    /// ⭐ **THE SIZE BOUND COUNTS THE ROWS IT WOULD SEND.** The browser refuses a capture of more
    /// than 2048 rows, and `qso_count` is not rows: it leaves out a logged dupe. Bounded on it, a
    /// Sweepstakes log of 2048 stations and one logged repeat went out as 2049 rows the page then
    /// refused, so the operator read "Station data unavailable" instead of "too large to show".
    #[test]
    fn the_size_bound_counts_the_rows_a_capture_carries() {
        let ss = |stations: u64| {
            let mut e = Engine::with_settings(crate::settings::Settings {
                mycall: "W1ABC".into(),
                fd_active: true,
                fd_event: "arrlss_cw".into(),
                contest_check: "68".into(),
                fd_section: "EMA".into(),
                contest_category_assisted: "NON-ASSISTED".into(),
                contest_category_power: "LOW".into(),
                ..Default::default()
            });
            e.restore_field_day_if_enabled();
            let Mode::FieldDay { station, .. } = &mut e.mode else {
                panic!("in SS")
            };
            // Every station once, then the first again: logged, marked, worth nothing.
            for (nr, i) in (0..stations).chain([0]).enumerate() {
                let call = format!("K1T{i}");
                let fields = [
                    ("NR", (nr + 1).to_string()),
                    ("PREC", "A".into()),
                    ("CALL", call.clone()),
                    ("CK", "72".into()),
                    ("SEC", "IL".into()),
                ]
                .map(|(k, v)| (k.to_string(), v));
                assert!(station
                    .log
                    .log_fields_at(&call, &fields, "CW", "", 0, 1000 + nr as u64));
            }
            e
        };
        let e = ss(2048);
        let native = e.snapshot().field_day.unwrap();
        assert_eq!(
            (native.qso_count, native.log.len(), native.log[2048].dupe),
            (2048, 2049, true),
            "PREMISE: the repeat is a row the count leaves out"
        );
        assert_eq!(
            e.bounded_field_day_status().map(|s| s.map(|s| s.log.len())),
            Err("applicationTooLarge"),
            "the station refuses a capture of 2049 rows rather than sending it"
        );
        // CONTROL: one station fewer is 2048 rows with the repeat, and goes whole.
        let e = ss(2047);
        let captured = e.bounded_field_day_status().unwrap().unwrap();
        assert_eq!((captured.qso_count, captured.log.len()), (2047, 2048));
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
