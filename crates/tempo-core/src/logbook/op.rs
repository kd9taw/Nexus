//! Typed log mutations, addressed by [`RecordId`] rather than by position.
//!
//! Every change the operator or a connector makes to the log is one [`LogOp`], applied by
//! [`Logbook::apply`], which reports back what it DID as [`Effects`]: the rows it added,
//! changed or removed, by id, and the [`OpClass`] of the change.
//!
//! **Why by id.** A position is only true of one revision. The UI asks to edit "row 3", and
//! between the asking and the writing another instance's append is folded in, a reconcile
//! reorders, or the operator deletes row 0 — and the edit lands on somebody else's contact.
//! Nothing in the old signature could express the difference, so nothing could catch it. An id
//! either names the row it was taken from or names nothing, and `Effects` says which happened.
//!
//! **Why Effects.** A derived index (worked entities, the awards fold, the dedup map) can be
//! UPDATED from the rows an op touched instead of rebuilt from the whole log, and the same
//! record is what a change journal would carry.
//!
//! Finding a row by id is a scan here. That is no worse than the match scans the stamp paths
//! already do, and the store that keeps an id → position index is where the index belongs —
//! it is the thing that can maintain one across a write.
//!
//! **One implementation per change** (SPEC-2 v3 §4.6). What an op does to the one row it names
//! is [`LogOp::apply_to`], a function of that row alone: [`Logbook::apply`] runs it on the row
//! it holds, and the station runs it on the row it read from the store, planning off the Engine
//! lock. Two callers, one body, so the two can never disagree about an edit.

use super::{Logbook, OpClass, QslVia, QsoRecord, RecordId, UploadStatus};

/// Which connector a stamp is for. `UploadState` holds the four as fields; an op has to be
/// able to name one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadService {
    Lotw,
    Eqsl,
    Qrz,
    Clublog,
}

/// What an op did, by id. Empty when it matched nothing — an id that names no row is not an
/// error, it is an answer (the row was deleted, or another instance holds it).
#[derive(Debug, Clone, PartialEq)]
pub struct Effects {
    /// What kind of change it was, and so what a cache keyed on a watermark must do about it.
    pub class: OpClass,
    pub added: Vec<RecordId>,
    pub changed: Vec<RecordId>,
    pub removed: Vec<RecordId>,
}

impl Effects {
    fn none(class: OpClass) -> Self {
        Self {
            class,
            added: Vec::new(),
            changed: Vec::new(),
            removed: Vec::new(),
        }
    }
    fn added(class: OpClass, ids: Vec<RecordId>) -> Self {
        Self {
            added: ids,
            ..Self::none(class)
        }
    }
    fn changed(class: OpClass, id: RecordId) -> Self {
        Self {
            changed: vec![id],
            ..Self::none(class)
        }
    }
    fn removed(class: OpClass, ids: Vec<RecordId>) -> Self {
        Self {
            removed: ids,
            ..Self::none(class)
        }
    }
    /// Whether the op touched a row at all.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}

/// One change to the log. The record-carrying variants are boxed: a `QsoRecord` is ~840 bytes,
/// and an enum is as big as its widest variant wherever it is passed.
#[derive(Debug, Clone)]
pub enum LogOp {
    /// One new contact. The caller has already decided it is not a duplicate.
    AppendOne(Box<QsoRecord>),
    /// Several new contacts, in order — a contest log merged into the general one.
    AppendRows(Vec<QsoRecord>),
    /// Correct a contact. Keeps the row's id and its QSL-sent mark; a callsign correction
    /// clears the confirmations and upload stamps the busted call earned (see
    /// [`Logbook::update_record`], which this is addressed by id instead of by position).
    Edit { id: RecordId, rec: Box<QsoRecord> },
    /// The operator says a card went out (or did not).
    MarkQslSent {
        id: RecordId,
        via: Option<QslVia>,
        date_unix: u64,
    },
    /// The operator says a paper card arrived (or did not) — a confirmation an awards fold
    /// reads, which is why it is not a [`Stamp`](LogOp::Stamp).
    MarkQslCard { id: RecordId, received: bool },
    /// The operator says this contact was — or was NOT — worked through a satellite
    /// (`PROP_MODE=SAT` + `SAT_NAME`). `Some(name)` tags it, `None` removes the tag.
    ///
    /// Its own op rather than a field of [`Edit`](LogOp::Edit) because REMOVAL has to be
    /// unmistakable. The edit form reads a blank field as "leave alone" so that a busted-call
    /// fix cannot strip a tag the contact earned; put the two fields on that form and a blank
    /// box means both "leave it" and "clear it" at once. Here the three states are distinct by
    /// construction: no op at all is leave-alone, `Some` sets, `None` clears. Same shape as
    /// [`MarkQslSent`](LogOp::MarkQslSent), whose `via: None` is the one value that withdraws.
    SetSatTag {
        id: RecordId,
        sat_name: Option<String>,
    },
    /// The operator says which DXCC entity this contact counts toward (ADIF `DXCC`): `Some`
    /// sets the number, `None` clears it and the contact counts by its call again. Its own op
    /// for [`SetSatTag`](LogOp::SetSatTag)'s reason: a clear must be unmistakable, and the
    /// edit form's blank means "leave alone".
    ///
    /// `country` is the ADIF `COUNTRY` the contact takes with the number the op leaves it — the
    /// name that number gives it, or its call's where it is left with none — named by the caller,
    /// which holds the entity table. `None` leaves `COUNTRY` as it is.
    SetDxcc {
        id: RecordId,
        dxcc: Option<u32>,
        country: Option<String>,
    },
    /// The operator sets — or removes, `None` — fields the log keeps as written
    /// ([`QsoRecord::extra`]: a county, a zone, a tag another program wrote), from the contact
    /// card. Its own op beside [`Edit`](LogOp::Edit), for [`SetSatTag`](LogOp::SetSatTag)'s
    /// reason: an edit's empty passthrough means "leave it", so removing the last such field
    /// could not be said there. Only fields that are nothing else ([`super::FieldEdit::check`]).
    SetFields {
        id: RecordId,
        fields: Vec<(String, Option<String>)>,
    },
    /// What happened the last time we pushed this contact to one connector.
    Stamp {
        id: RecordId,
        service: UploadService,
        status: UploadStatus,
    },
    /// Remove a mis-logged contact.
    Delete(RecordId),
    /// Remove every contact (the operator-confirmed purge).
    Clear,
}

/// What one op makes of the one row it names — [`LogOp::apply_to`].
#[derive(Debug, Clone, PartialEq)]
pub enum RowAfter {
    /// The row as the op leaves it (boxed: a row is large, and the other answers carry nothing).
    Now(Box<QsoRecord>),
    /// The op removes the row (a delete).
    Gone,
    /// The op changes nothing here: it refuses this row (a satellite tag with a blank name), or
    /// it names no one row at all (an append, a purge).
    Unchanged,
}

impl LogOp {
    /// The id of the one row this op changes — `None` for the ops that name no one row (an
    /// append, a purge).
    pub fn target(&self) -> Option<RecordId> {
        match self {
            LogOp::Edit { id, .. }
            | LogOp::MarkQslSent { id, .. }
            | LogOp::MarkQslCard { id, .. }
            | LogOp::SetSatTag { id, .. }
            | LogOp::SetDxcc { id, .. }
            | LogOp::SetFields { id, .. }
            | LogOp::Stamp { id, .. }
            | LogOp::Delete(id) => Some(*id),
            LogOp::AppendOne(_) | LogOp::AppendRows(_) | LogOp::Clear => None,
        }
    }

    /// ★ What this op makes of `row`, the row it names — THE one implementation of every change
    /// to one contact (SPEC-2 v3 §4.6). The in-memory log runs it on the row it holds
    /// ([`Logbook::apply`]); the station runs it on the row it read from the store, off the
    /// Engine lock, and commits what it answers under the lock. The rules of each change live in
    /// the function it calls, and nowhere else: [`super::edited`], [`super::qsl_sent_marked`],
    /// [`super::qsl_card_marked`], [`super::sat_tagged`], [`super::dxcc_set`], [`super::fields_set`],
    /// [`super::stamped`].
    ///
    /// It never looks at `row`'s id: the caller found the row by it.
    pub fn apply_to(&self, row: &QsoRecord) -> RowAfter {
        let mut now = row.clone();
        match self {
            LogOp::Edit { rec, .. } => {
                return RowAfter::Now(Box::new(super::edited(row, rec.as_ref().clone())))
            }
            LogOp::MarkQslSent { via, date_unix, .. } => {
                super::qsl_sent_marked(&mut now, *via, *date_unix)
            }
            LogOp::MarkQslCard { received, .. } => super::qsl_card_marked(&mut now, *received),
            LogOp::SetSatTag { sat_name, .. } => {
                if !super::sat_tagged(&mut now, sat_name.as_deref()) {
                    return RowAfter::Unchanged;
                }
            }
            LogOp::SetDxcc { dxcc, country, .. } => {
                super::dxcc_set(&mut now, *dxcc, country.as_deref())
            }
            LogOp::SetFields { fields, .. } => super::fields_set(&mut now, fields),
            LogOp::Stamp {
                service, status, ..
            } => super::stamped(&mut now, *service, status.clone()),
            LogOp::Delete(_) => return RowAfter::Gone,
            LogOp::AppendOne(_) | LogOp::AppendRows(_) | LogOp::Clear => {
                return RowAfter::Unchanged
            }
        }
        RowAfter::Now(Box::new(now))
    }

    /// What applying this op will cost, before it is applied — the class is a property of the
    /// op, not of what it happens to find.
    pub fn class(&self) -> OpClass {
        match self {
            LogOp::AppendOne(_) | LogOp::AppendRows(_) => OpClass::Append,
            LogOp::Edit { .. } => OpClass::Key,
            LogOp::MarkQslSent { .. } | LogOp::Stamp { .. } => OpClass::Stamp,
            LogOp::MarkQslCard { .. } => OpClass::Upgrade,
            // `Key`, not the narrower `Upgrade` its neighbour gets: a tag can be REMOVED, and
            // `Upgrade` promises a monotone change a plan built on an earlier snapshot can be
            // re-applied over. Naming a change wider than it is costs a rebuild; naming it
            // narrower serves a stale answer — and the awards fold reads `PROP_MODE` for the
            // Satellite-VUCC split, so a stale answer here is a wrong award. Exactly what the
            // same change through `Edit` would cost, on an act an operator performs by hand.
            LogOp::SetSatTag { .. } => OpClass::Key,
            // `Key` for the tag's reason: it can be cleared, and every fold that names a contact's
            // entity reads `DXCC` — the awards, the needs, the statistics, the Logbook's country
            // column and its order — so a narrower class would serve them a stale entity.
            LogOp::SetDxcc { .. } => OpClass::Key,
            // `Key` for the same reason: a field can be removed, and the passthrough holds what
            // the awards read (the zones, a county, LoTW's own DXCC).
            LogOp::SetFields { .. } => OpClass::Key,
            LogOp::Delete(_) | LogOp::Clear => OpClass::Structural,
        }
    }
}

impl Logbook {
    /// Apply one [`LogOp`] and report what it did. Pure: no I/O, no clock, no network — the
    /// caller persists what the effects name.
    pub fn apply(&mut self, op: LogOp) -> Effects {
        let class = op.class();
        match op {
            LogOp::AppendOne(rec) => Effects::added(class, vec![self.add(*rec)]),
            LogOp::AppendRows(rows) => {
                Effects::added(class, rows.into_iter().map(|r| self.add(r)).collect())
            }
            LogOp::Edit { .. }
            | LogOp::MarkQslSent { .. }
            | LogOp::MarkQslCard { .. }
            | LogOp::SetSatTag { .. }
            | LogOp::SetDxcc { .. }
            | LogOp::SetFields { .. }
            | LogOp::Stamp { .. }
            | LogOp::Delete(_) => {
                let Some(id) = op.target() else {
                    return Effects::none(class);
                };
                let Some(i) = self.position_of(id) else {
                    return Effects::none(class);
                };
                match op.apply_to(&self.records()[i]) {
                    RowAfter::Now(row) => {
                        self.records_mut(class)[i] = std::sync::Arc::new(*row);
                        Effects::changed(class, id)
                    }
                    RowAfter::Gone => {
                        self.records.write_as(class).remove(i);
                        Effects::removed(class, vec![id])
                    }
                    RowAfter::Unchanged => Effects::none(class),
                }
            }
            LogOp::Clear => {
                let ids: Vec<RecordId> = self.records().iter().filter_map(|r| r.id).collect();
                self.clear();
                Effects::removed(class, ids)
            }
        }
    }

    /// Where the row with this id is. A scan — see the module header for why that is the right
    /// answer here and the wrong one in the store.
    fn position_of(&self, id: RecordId) -> Option<usize> {
        self.records().iter().position(|r| r.id == Some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{UploadOutcome, UploadState};

    fn rec_of(call: &str, when: u64) -> QsoRecord {
        QsoRecord {
            id: None,
            call: call.into(),
            grid: Some("EN37".into()),
            country: None,
            state: None,
            band: "20m".into(),
            freq_mhz: 14.074,
            freq_rx_mhz: None,
            mode: "FT8".into(),
            rst_sent: Some("-10".into()),
            rst_rcvd: Some("-12".into()),
            name: None,
            comment: None,
            notes: None,
            qth: None,
            tx_power: None,
            when_unix: when,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: UploadState::default(),
            ota: Default::default(),
            time_known: true,
            dxcc: None,
            prop_mode: None,
            sat_name: None,
            operator: None,
            my_grid: None,
            my_rig: None,
            station_callsign: None,
            extra: Vec::new(),
            contest: None,
        }
    }

    fn seeded(calls: &[&str]) -> (Logbook, Vec<RecordId>) {
        let mut lb = Logbook::new();
        let ids = calls
            .iter()
            .enumerate()
            .map(|(i, c)| lb.add(rec_of(c, 1_700_000_000 + i as u64)))
            .collect();
        (lb, ids)
    }

    fn call_at(lb: &Logbook, i: usize) -> String {
        lb.records()[i].call.clone()
    }

    /// ★ THE REASON ANY OF THIS EXISTS. The operator opens the edit form on a contact, and
    /// before it is submitted the row it sits at stops being that contact — another instance's
    /// append is folded in, a reconcile reorders, or an earlier row is deleted. Addressed by
    /// POSITION the edit lands on a stranger's QSO and nothing can tell; addressed by id it
    /// lands on the contact it was opened on, or on nothing.
    #[test]
    fn a_change_follows_its_contact_when_the_rows_move_under_it() {
        let (mut lb, ids) = seeded(&["W1AW", "K5XYZ", "DL1ABC", "JA1XYZ"]);
        let target = ids[3];
        assert_eq!(call_at(&lb, 3), "JA1XYZ");

        // Two rows leave: what was row 3 is now row 1.
        assert_eq!(lb.apply(LogOp::Delete(ids[0])).removed, vec![ids[0]]);
        assert_eq!(lb.apply(LogOp::Delete(ids[1])).removed, vec![ids[1]]);
        assert_eq!(call_at(&lb, 1), "JA1XYZ", "the row moved");

        let mut fixed = lb.records()[1].as_ref().clone();
        fixed.call = "JA1XYZ/P".into();
        let done = lb.apply(LogOp::Edit {
            id: target,
            rec: Box::new(fixed),
        });
        assert_eq!(done.changed, vec![target], "the contact, not the position");
        assert_eq!(call_at(&lb, 1), "JA1XYZ/P");
        assert_eq!(call_at(&lb, 0), "DL1ABC", "and nobody else was touched");
        assert_eq!(
            lb.records()[1].id,
            Some(target),
            "an edit keeps the row's identity"
        );
    }

    /// An id that names no row — the contact was deleted while a stamp was in flight, which is
    /// exactly what the upload worker does — changes nothing and says so. It must not fall
    /// back to a position, or to "the newest match", or stamp a stranger.
    #[test]
    fn an_op_for_a_row_that_is_gone_changes_nothing_and_says_so() {
        let (mut lb, ids) = seeded(&["W1AW", "K5XYZ"]);
        let gone = ids[0];
        lb.apply(LogOp::Delete(gone));
        let before: Vec<_> = lb.records().iter().map(|r| r.as_ref().clone()).collect();

        for op in [
            LogOp::Edit {
                id: gone,
                rec: Box::new(rec_of("N0BODY", 1_700_000_900)),
            },
            LogOp::Stamp {
                id: gone,
                service: UploadService::Lotw,
                status: UploadStatus {
                    outcome: UploadOutcome::Accepted,
                    when_unix: 1_700_000_500,
                    detail: None,
                },
            },
            LogOp::MarkQslCard {
                id: gone,
                received: true,
            },
            LogOp::MarkQslSent {
                id: gone,
                via: Some(QslVia::Bureau),
                date_unix: 1_700_000_500,
            },
            // Both directions: a tag set AND a tag removed must miss a row that is gone.
            // The clear is the one that would hurt — it would strip a stranger's tag.
            LogOp::SetSatTag {
                id: gone,
                sat_name: Some("AO-91".into()),
            },
            LogOp::SetSatTag {
                id: gone,
                sat_name: None,
            },
            // The same for a DXCC number set and cleared.
            LogOp::SetDxcc {
                id: gone,
                dxcc: Some(182),
                country: Some("Navassa Island".into()),
            },
            LogOp::SetDxcc {
                id: gone,
                dxcc: None,
                country: None,
            },
            LogOp::Delete(gone),
        ] {
            let effects = lb.apply(op);
            assert!(effects.is_empty(), "{effects:?} for a row that is gone");
            let now: Vec<_> = lb.records().iter().map(|r| r.as_ref().clone()).collect();
            assert_eq!(now, before, "and no other row was touched");
        }
    }

    /// Each op reports the ids it touched and the class it costs, and the class it declares
    /// BEFORE it runs is the one it reports after — a store has to plan on the former.
    #[test]
    fn every_op_reports_the_rows_it_touched_and_the_class_it_costs() {
        let status = UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_700_000_500,
            detail: None,
        };
        // (name, the op built against the seeded ids, the class it must cost)
        type Case = (&'static str, fn(&[RecordId]) -> LogOp, OpClass);
        let cases: Vec<Case> = vec![
            (
                "append",
                |_| LogOp::AppendOne(Box::new(rec_of("VK2ABC", 1_700_000_800))),
                OpClass::Append,
            ),
            (
                "append rows",
                |_| LogOp::AppendRows(vec![rec_of("VK3ABC", 1_700_000_801)]),
                OpClass::Append,
            ),
            (
                "edit",
                |ids| LogOp::Edit {
                    id: ids[1],
                    rec: Box::new(rec_of("K5XYZ", 1_700_000_001)),
                },
                OpClass::Key,
            ),
            (
                "qsl sent",
                |ids| LogOp::MarkQslSent {
                    id: ids[1],
                    via: Some(QslVia::Direct),
                    date_unix: 1_700_000_500,
                },
                OpClass::Stamp,
            ),
            (
                "qsl card",
                |ids| LogOp::MarkQslCard {
                    id: ids[1],
                    received: true,
                },
                OpClass::Upgrade,
            ),
            (
                "sat tag",
                |ids| LogOp::SetSatTag {
                    id: ids[1],
                    sat_name: Some("AO-91".into()),
                },
                OpClass::Key,
            ),
            (
                "sat untag",
                |ids| LogOp::SetSatTag {
                    id: ids[1],
                    sat_name: None,
                },
                OpClass::Key,
            ),
            (
                "dxcc",
                |ids| LogOp::SetDxcc {
                    id: ids[1],
                    dxcc: Some(182),
                    country: Some("Navassa Island".into()),
                },
                OpClass::Key,
            ),
            (
                "dxcc clear",
                |ids| LogOp::SetDxcc {
                    id: ids[1],
                    dxcc: None,
                    country: None,
                },
                OpClass::Key,
            ),
            ("delete", |ids| LogOp::Delete(ids[1]), OpClass::Structural),
            ("clear", |_| LogOp::Clear, OpClass::Structural),
        ];
        for (name, build, class) in cases {
            let (mut lb, ids) = seeded(&["W1AW", "K5XYZ"]);
            let op = build(&ids);
            assert_eq!(op.class(), class, "{name}: the class it declares");
            let effects = lb.apply(op);
            assert_eq!(effects.class, class, "{name}: ...and the one it reports");
            assert!(!effects.is_empty(), "{name}: it touched a row");
            match name {
                "append" | "append rows" => assert_eq!(effects.added.len(), 1),
                "delete" => assert_eq!(effects.removed, vec![ids[1]]),
                "clear" => assert_eq!(effects.removed, ids, "every row, in log order"),
                _ => assert_eq!(effects.changed, vec![ids[1]]),
            }
        }

        // The stamp lands on the service it names, and on no other.
        let (mut lb, ids) = seeded(&["W1AW", "K5XYZ"]);
        let effects = lb.apply(LogOp::Stamp {
            id: ids[1],
            service: UploadService::Clublog,
            status: status.clone(),
        });
        assert_eq!(effects.changed, vec![ids[1]]);
        assert_eq!(effects.class, OpClass::Stamp);
        let row = &lb.records()[1];
        assert_eq!(row.upload.clublog, Some(status));
        assert_eq!(row.upload.lotw, None, "no other service was stamped");
        assert_eq!(row.upload.eqsl, None);
        assert_eq!(row.upload.qrz, None);
    }

    /// The DXCC number set by hand: it lands on the one row it names; a clear takes it off; and
    /// LoTW's number, kept beside the log's where LoTW confirmed the contact as another entity,
    /// is never overwritten — a number that agrees with it drops it, and a clear leaves the log
    /// holding it, as a LoTW download leaves a contact that had none.
    #[test]
    fn a_dxcc_set_by_hand_keeps_lotws_number_beside_it() {
        use crate::logbook::{record_lotw_dxcc, LOTW_DXCC_FIELD};
        let (mut lb, ids) = seeded(&["K1N", "K1B"]);
        let at = |lb: &Logbook, i: usize| {
            let r = &lb.records()[i];
            (r.dxcc, record_lotw_dxcc(r))
        };
        let set = |lb: &mut Logbook, dxcc| {
            lb.apply(LogOp::SetDxcc {
                id: ids[0],
                dxcc,
                country: None,
            });
        };
        set(&mut lb, Some(182));
        assert_eq!(at(&lb, 0), (Some(182), None), "set");
        assert_eq!(at(&lb, 1), (None, None), "and nobody else was touched");
        set(&mut lb, None);
        assert_eq!(at(&lb, 0), (None, None), "cleared: the call counts again");

        // LoTW confirmed the contact as Navassa, the log holding the United States.
        let lotw_kept = |lb: &mut Logbook| {
            let mut r = lb.records()[0].as_ref().clone();
            r.dxcc = Some(291);
            r.extra = vec![(LOTW_DXCC_FIELD.to_string(), "182".to_string())];
            lb.records_mut(OpClass::Key)[0] = std::sync::Arc::new(r);
        };
        lotw_kept(&mut lb);
        set(&mut lb, Some(79));
        assert_eq!(
            at(&lb, 0),
            (Some(79), Some(182)),
            "LoTW's is kept beside another"
        );
        set(&mut lb, Some(182));
        assert_eq!(at(&lb, 0), (Some(182), None), "one that agrees drops it");
        assert!(lb.records()[0].extra.is_empty(), "the field itself goes");
        lotw_kept(&mut lb);
        set(&mut lb, None);
        assert_eq!(
            at(&lb, 0),
            (Some(182), None),
            "a clear leaves the log LoTW's"
        );
    }

    /// The contact's COUNTRY goes with the number set by hand: the name the caller gives for the
    /// number the op leaves is written beside it, on a set and on a clear alike, and an op that
    /// names none leaves COUNTRY as it was.
    #[test]
    fn a_dxcc_set_by_hand_writes_the_country_its_caller_names() {
        let (mut lb, ids) = seeded(&["K1N", "K1B"]);
        let mut first = lb.records()[0].as_ref().clone();
        first.country = Some("United States".into());
        lb.records_mut(OpClass::Key)[0] = std::sync::Arc::new(first);
        let held = |lb: &Logbook, i: usize| {
            let r = &lb.records()[i];
            (r.dxcc, r.country.clone())
        };
        let set = |lb: &mut Logbook, dxcc, country: Option<&str>| {
            lb.apply(LogOp::SetDxcc {
                id: ids[0],
                dxcc,
                country: country.map(str::to_string),
            });
        };
        set(&mut lb, Some(182), Some("Navassa Island"));
        assert_eq!(
            held(&lb, 0),
            (Some(182), Some("Navassa Island".into())),
            "set: the two agree"
        );
        assert_eq!(held(&lb, 1), (None, None), "nobody else was touched");
        set(&mut lb, None, Some("United States"));
        assert_eq!(
            held(&lb, 0),
            (None, Some("United States".into())),
            "cleared: the call's entity again"
        );
        set(&mut lb, Some(182), None);
        assert_eq!(
            held(&lb, 0),
            (Some(182), Some("United States".into())),
            "no name given: COUNTRY as it was"
        );
    }

    /// An appended row leaves with an id, and the effects hand back the same one the log gave
    /// it — the caller's copy has to be able to address it later.
    #[test]
    fn an_appended_row_hands_back_the_id_the_log_gave_it() {
        let mut lb = Logbook::new();
        let effects = lb.apply(LogOp::AppendRows(vec![
            rec_of("W1AW", 1_700_000_000),
            rec_of("K5XYZ", 1_700_000_001),
        ]));
        assert_eq!(effects.added.len(), 2);
        let held: Vec<RecordId> = lb.records().iter().map(|r| r.id.unwrap()).collect();
        assert_eq!(effects.added, held, "in the order they were appended");
    }

    /// ★ `SetFields`: sets, corrects and removes fields kept as written, in name order, touching
    /// nothing else on the row — and can take off the last one, which an edit's empty
    /// passthrough ("leave it") cannot say.
    #[test]
    fn set_fields_changes_exactly_the_named_passthrough_fields() {
        let (_, ids) = seeded(&["K1ABC"]);
        let id = ids[0];
        let mut row = rec_of("K1ABC", 1_700_000_000);
        row.extra = vec![
            ("CNTY".into(), "MA,MIDDLESEX".into()),
            ("CQZ".into(), "5".into()),
        ];
        let op = LogOp::SetFields {
            id,
            fields: vec![
                ("CNTY".into(), Some("MA,ESSEX".into())),
                ("CQZ".into(), None),
                ("ITUZ".into(), Some("8".into())),
            ],
        };
        let RowAfter::Now(now) = op.apply_to(&row) else {
            panic!("a change")
        };
        let mut want = row.clone();
        want.extra = vec![
            ("CNTY".into(), "MA,ESSEX".into()),
            ("ITUZ".into(), "8".into()),
        ];
        assert_eq!(*now, want);
        assert_eq!(op.class(), OpClass::Key);

        let last = LogOp::SetFields {
            id,
            fields: vec![("CNTY".into(), None), ("ITUZ".into(), None)],
        };
        let RowAfter::Now(empty) = last.apply_to(&now) else {
            panic!("a change")
        };
        assert!(empty.extra.is_empty(), "the last field goes too");
    }
}
