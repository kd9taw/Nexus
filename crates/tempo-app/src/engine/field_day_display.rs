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
                    dkey: rs.dupe_rule.key(q),
                    // ⭐ The engine's own answer about this row, so the log table marks a
                    // zero-scoring dupe instead of guessing at one from a repeated callsign.
                    dupe: q.dupe,
                })
                .collect(),
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
            // which is the rule for two of the seventeen shipped rulesets.
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
                    // The generalised keys now ship too, and they are the SAME contacts
                    // under a wider key — so they get the same count bound. Unbounded,
                    // a host could grow the capture past the limit the triple enforces.
                    || self.fd_mirror.dkeys.len() > 4096
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
        // …and the one string rendered from them, which is bounded by the browser's own
        // per-string rule and must be refused here first rather than there.
        check(&self.contest_sent_exchange().unwrap_or_default())?;
        check(&self.contest_composing_text().unwrap_or_default())?;
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
    /// the seventeen shipped rulesets. Sweepstakes keys on the CALL ALONE (rule 2.2 —
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
        // …and Field Day REFUSES a dupe rather than logging one, which is what the card's
        // wording turns on: telling this operator the contact would be refused is true here.
        assert_eq!(wire["dupeRule"]["logDupes"], false);

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

    /// ⭐ **A LOGGED DUPE MUST SAY SO ON THE WIRE.** In the seven cross-checked contests a
    /// duplicate is now logged and scored zero rather than refused, so the contest log table
    /// gained rows that score nothing. Without a flag on the row the operator reviewing their
    /// log after the event cannot tell a zero-scoring dupe from a real contact, cannot check
    /// the sponsor's math against their own, and cannot tell an intended dupe from a logging
    /// mistake — an ambiguity that did not exist while a dupe was never logged at all.
    ///
    /// The UI must not re-derive this. It had been doing exactly that, over a fourth copy of
    /// the key, which is what the row flag replaces.
    #[test]
    fn a_logged_dupe_says_so_on_the_row_and_field_day_still_has_none_to_say_it_about() {
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

        // CONTROL — Field Day REFUSES a dupe rather than logging one, so there is no marked
        // row to find and the flag stays false. This is the half that must never move: if it
        // ever reports a dupe row, Field Day has started logging them.
        let mut e = engine("arrlfd", 1);
        let Mode::FieldDay { station, .. } = &mut e.mode else {
            panic!("active")
        };
        let before = station.log.qso_count();
        assert!(!station.log.log_mode_at("K1T0", "2A", "WI", "PH", 99, 1000));
        let wire = serde_json::to_value(e.snapshot().field_day.unwrap()).unwrap();
        assert_eq!(wire["log"].as_array().unwrap().len(), before);
        assert!(wire["log"][0].get("dupe").is_none(), "{}", wire["log"][0]);
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
