//! What an operator edit of one logged contact may change, and the key that notices somebody
//! else changing it first.
//!
//! [`QsoEdit`] is the edit form's payload AND the field list the concurrency key is computed
//! over — deliberately one type rather than two, because the failure mode of two is silent.
//! A new editable field added to a hand-written key list is a field the key does not cover,
//! so an edit of it can be lost to a concurrent write with no refusal and no message; here a
//! new field joins the key by existing. `qso-edit-fields.json` pins the resulting shape so
//! the form on the other side of the wire is checked against the same list.
//!
//! **Why the key is narrower than the row.** Today an edit is refused when the row it opened
//! differs from the stored row in ANY field, which includes fields no operator touched: an
//! upload stamp landing between opening the form and saving it (three services on auto-upload,
//! one stamp each) refuses an edit that conflicts with nothing. The key is computed over what
//! an edit can actually write, so a background stamp, a confirmation arriving from LoTW and
//! granted award credit all leave it where it was. A change that CAN collide — an ADIF import
//! filling in a blank `STATE`, another instance correcting the call — does move it, and the
//! refusal carries the current row so the operator can see what happened and retry.

use super::id::fnv1a64;
use super::QsoRecord;
use serde::{Deserialize, Serialize};

/// Exactly what the Logbook's edit form writes to one contact.
///
/// ⚠️ **Every field is serialized, including a `None`** — no `skip_serializing_if` may be
/// added here. A skipped field is absent from the JSON the key hashes, so clearing that field
/// would produce the same key as never having had it, and the concurrency check would go blind
/// to exactly the edit that removed something.
///
/// Field order is declaration order (serde writes a struct in the order it is written), which
/// is what makes the serialization canonical without a sorting pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QsoEdit {
    pub call: String,
    pub grid: Option<String>,
    pub state: Option<String>,
    pub band: String,
    pub freq_mhz: f64,
    pub mode: String,
    pub rst_sent: Option<String>,
    pub rst_rcvd: Option<String>,
    pub name: Option<String>,
    pub qth: Option<String>,
    pub comment: Option<String>,
    pub notes: Option<String>,
    pub tx_power: Option<f64>,
    /// Contact START time (ADIF `QSO_DATE`/`TIME_ON`), Unix seconds UTC.
    pub when_unix: u64,
    /// Contact END time (ADIF `QSO_DATE_OFF`/`TIME_OFF`), Unix seconds UTC.
    ///
    /// `None` means LEAVE ALONE on the way in — [`super::Logbook::update_record`] restores the
    /// stored value — so the form corrects an end time and never clears one. It is in the key
    /// all the same: on the way OUT it is projected from the record, so two rows that differ
    /// in their stored end time have different keys.
    pub time_off_unix: Option<u64>,
    pub ota: OtaEdit,
    /// The operator's own grid and rig for this contact (ADIF `MY_GRIDSQUARE` / `MY_RIG`).
    pub my_grid: Option<String>,
    pub my_rig: Option<String>,
    /// The outbound QSL-sent mark in the vocabulary the form uses: `None` for not sent, `"B"`,
    /// `"D"` or `"E"` for a recorded method, and `"SENT"` for a mark carrying no method.
    ///
    /// In the key because the form writes it — `edit_qso` applies the field edit and the two
    /// QSL marks in one commit — and it is safe there because nothing in the background sets
    /// it: a QSL SENT mark is operator-declared, unlike every upload stamp and every inbound
    /// confirmation.
    pub qsl_sent_via: Option<String>,
    /// Whether a paper QSL card has arrived (ADIF `QSL_RCVD`).
    ///
    /// The one confirmation channel in the key, and it belongs here for the same reason: a
    /// card is operator-declared. LoTW, eQSL and QRZ write their own channels, never this one,
    /// so a sync cannot move the key. An ADIF import carrying `QSL_RCVD` can, and that is the
    /// intended reading — it is a change an operator's edit would otherwise overwrite.
    pub qsl_card: bool,
}

/// The park/summit half of an edit (ADIF `MY_SIG`/`MY_SIG_INFO`/`SIG`/`SIG_INFO`, `*_SOTA_REF`).
///
/// ⚠️ [`super::Ota::iota`] is deliberately absent. The form carries the stored value straight
/// back out rather than offering a box for it, so it is not something an edit can change, and
/// a field in the key that no edit can move only widens the refusal surface.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OtaEdit {
    pub my_program: Option<String>,
    pub my_ref: Option<String>,
    pub their_program: Option<String>,
    pub their_ref: Option<String>,
}

impl QsoEdit {
    /// The editable projection of a stored record — the other half of the key's definition.
    pub fn project(r: &QsoRecord) -> QsoEdit {
        QsoEdit {
            call: r.call.clone(),
            grid: r.grid.clone(),
            state: r.state.clone(),
            band: r.band.clone(),
            freq_mhz: r.freq_mhz,
            mode: r.mode.clone(),
            rst_sent: r.rst_sent.clone(),
            rst_rcvd: r.rst_rcvd.clone(),
            name: r.name.clone(),
            qth: r.qth.clone(),
            comment: r.comment.clone(),
            notes: r.notes.clone(),
            tx_power: r.tx_power,
            when_unix: r.when_unix,
            time_off_unix: r.time_off_unix,
            ota: OtaEdit {
                my_program: r.ota.my_program.clone(),
                my_ref: r.ota.my_ref.clone(),
                their_program: r.ota.their_program.clone(),
                their_ref: r.ota.their_ref.clone(),
            },
            my_grid: r.my_grid.clone(),
            my_rig: r.my_rig.clone(),
            qsl_sent_via: match (r.qsl_sent.sent, r.qsl_sent.via) {
                (false, _) => None,
                (true, Some(via)) => Some(via.code().to_string()),
                // Sent, but the method was never recorded (a legacy row, or an import whose
                // `QSL_SENT_VIA` was absent). "SENT" is the form's own token for that state;
                // reading it as "not sent" would let an edit silently withdraw the mark.
                (true, None) => Some("SENT".to_string()),
            },
            qsl_card: r.qsl_rcvd.card,
        }
    }

    /// The key an edit is checked against: FNV-1a-64, hex, over this projection's JSON.
    ///
    /// Computed here and never in the UI — the UI is handed the key with the row and sends it
    /// back, so there is one implementation of the field list and one of the hash.
    ///
    /// JSON carries no NaN and no infinity, and serde_json writes any non-finite float as
    /// `null` rather than refusing — measured, not assumed. So the key is total for every
    /// record, at the cost of a NaN frequency and an infinite one hashing alike. Neither can
    /// reach here (the form parses the field behind a NaN guard, and the ADIF writer already
    /// declines to emit a non-finite frequency), so that collision is not worth a normalising
    /// pass that would itself have to be kept honest.
    pub fn key(&self) -> String {
        let json = serde_json::to_vec(self).expect("a struct with no map keys always serializes");
        format!("{:016x}", fnv1a64(&json))
    }

    /// The serde field paths of an edit, `ota.myRef` style, sorted.
    ///
    /// Read back off the serialized form rather than listed, so that a field added to the
    /// struct appears here without anyone remembering to add it — which is the whole point of
    /// the golden this feeds.
    ///
    /// Sorted EXPLICITLY rather than left in serde's order: `serde_json::Value` holds a
    /// `BTreeMap` today, but its `preserve_order` feature would make it insertion-ordered, and
    /// a cargo feature is unified across the whole workspace — so a crate none of this knows
    /// about could otherwise reorder this golden. Order carries no meaning here anyway; the
    /// golden is a SET comparison, and the form on the other side sorts its payload paths too.
    pub fn field_paths(&self) -> Vec<String> {
        fn walk(v: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
            let Some(map) = v.as_object() else { return };
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if v.is_object() {
                    walk(v, &path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        let value = serde_json::to_value(self).expect("QsoEdit serializes to an object");
        walk(&value, "", &mut out);
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{Ota, QslRcvd, QslSent, QslVia, UploadState};

    /// The shared TS↔Rust golden: the field list the edit form and the key must agree on.
    const FIELDS_GOLDEN: &str =
        include_str!("../../../../ui/src/features/__fixtures__/log-query/qso-edit-fields.json");

    fn record() -> QsoRecord {
        QsoRecord {
            id: None,
            call: "DL1ABC".into(),
            grid: Some("JO31".into()),
            country: Some("Fed. Rep. of Germany".into()),
            state: None,
            band: "20m".into(),
            freq_mhz: 14.074,
            freq_rx_mhz: None,
            mode: "FT8".into(),
            rst_sent: Some("-12".into()),
            rst_rcvd: Some("-09".into()),
            name: Some("Hans".into()),
            qth: Some("Koln".into()),
            comment: Some("tnx".into()),
            notes: Some("dipole".into()),
            tx_power: Some(50.0),
            when_unix: 1_758_000_000,
            time_known: true,
            time_off_unix: Some(1_758_000_120),
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: QslRcvd::default(),
            qsl_sent: QslSent::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: UploadState::default(),
            ota: Ota {
                my_program: Some("POTA".into()),
                my_ref: Some("K-1234".into()),
                their_program: None,
                their_ref: None,
                iota: Some("NA-001".into()),
            },
            dxcc: Some(230),
            prop_mode: None,
            sat_name: None,
            operator: None,
            station_callsign: None,
            my_grid: Some("EN52".into()),
            my_rig: Some("FT-710".into()),
            extra: Vec::new(),
            contest: None,
        }
    }

    /// R2's guard: the field list is the struct, and the golden the UI form is checked against
    /// is checked against the struct too. A field added to `QsoEdit` without the form learning
    /// about it — or removed from it while the form still sends it — reddens here.
    #[test]
    fn qso_edit_fields_match_the_golden() {
        let golden: Vec<String> =
            serde_json::from_str(FIELDS_GOLDEN).expect("the golden is a JSON array of strings");
        assert_eq!(QsoEdit::project(&record()).field_paths(), golden);
    }

    /// The golden would be satisfied by an empty struct if `None` were skipped, so pin that a
    /// record with nothing filled in still carries every path.
    #[test]
    fn every_field_is_serialized_even_when_it_is_empty() {
        let mut r = record();
        r.grid = None;
        r.state = None;
        r.rst_sent = None;
        r.rst_rcvd = None;
        r.name = None;
        r.qth = None;
        r.comment = None;
        r.notes = None;
        r.tx_power = None;
        r.time_off_unix = None;
        r.ota = Ota::default();
        r.my_grid = None;
        r.my_rig = None;
        let golden: Vec<String> = serde_json::from_str(FIELDS_GOLDEN).expect("golden parses");
        assert_eq!(QsoEdit::project(&r).field_paths(), golden);
    }

    /// Every field in the key is a field the key can SEE change. Written as a sweep over the
    /// projection rather than a list of cases, so a new field is covered by the test that
    /// enumerates the golden rather than by a case somebody has to remember to write.
    #[test]
    fn edit_key_moves_for_every_edit_field() {
        let base = record();
        let base_key = QsoEdit::project(&base).key();
        /// One edit of one field, labelled with the golden path it is meant to move.
        type Mutation = (&'static str, Box<dyn Fn(&mut QsoRecord)>);
        // One mutation per golden path, named by the path it moves, in the golden's own order.
        let mutations: Vec<Mutation> = vec![
            ("band", Box::new(|r: &mut QsoRecord| r.band = "40m".into())),
            (
                "call",
                Box::new(|r: &mut QsoRecord| r.call = "DL2XYZ".into()),
            ),
            (
                "comment",
                Box::new(|r: &mut QsoRecord| r.comment = Some("73".into())),
            ),
            ("freqMhz", Box::new(|r: &mut QsoRecord| r.freq_mhz = 7.074)),
            (
                "grid",
                Box::new(|r: &mut QsoRecord| r.grid = Some("JO32".into())),
            ),
            ("mode", Box::new(|r: &mut QsoRecord| r.mode = "CW".into())),
            (
                "myGrid",
                Box::new(|r: &mut QsoRecord| r.my_grid = Some("EN53".into())),
            ),
            (
                "myRig",
                Box::new(|r: &mut QsoRecord| r.my_rig = Some("IC-7300".into())),
            ),
            (
                "name",
                Box::new(|r: &mut QsoRecord| r.name = Some("Klaus".into())),
            ),
            (
                "notes",
                Box::new(|r: &mut QsoRecord| r.notes = Some("vertical".into())),
            ),
            (
                "ota.myProgram",
                Box::new(|r: &mut QsoRecord| r.ota.my_program = Some("SOTA".into())),
            ),
            (
                "ota.myRef",
                Box::new(|r: &mut QsoRecord| r.ota.my_ref = Some("K-9999".into())),
            ),
            (
                "ota.theirProgram",
                Box::new(|r: &mut QsoRecord| r.ota.their_program = Some("POTA".into())),
            ),
            (
                "ota.theirRef",
                Box::new(|r: &mut QsoRecord| r.ota.their_ref = Some("K-4321".into())),
            ),
            (
                "qslCard",
                Box::new(|r: &mut QsoRecord| r.qsl_rcvd.card = true),
            ),
            (
                "qslSentVia",
                Box::new(|r: &mut QsoRecord| {
                    r.qsl_sent.sent = true;
                    r.qsl_sent.via = Some(QslVia::Bureau);
                }),
            ),
            (
                "qth",
                Box::new(|r: &mut QsoRecord| r.qth = Some("Bonn".into())),
            ),
            (
                "rstRcvd",
                Box::new(|r: &mut QsoRecord| r.rst_rcvd = Some("599".into())),
            ),
            (
                "rstSent",
                Box::new(|r: &mut QsoRecord| r.rst_sent = Some("599".into())),
            ),
            (
                "state",
                Box::new(|r: &mut QsoRecord| r.state = Some("IL".into())),
            ),
            (
                "timeOffUnix",
                Box::new(|r: &mut QsoRecord| r.time_off_unix = None),
            ),
            (
                "txPower",
                Box::new(|r: &mut QsoRecord| r.tx_power = Some(100.0)),
            ),
            ("whenUnix", Box::new(|r: &mut QsoRecord| r.when_unix += 1)),
        ];

        // The sweep is only as good as its coverage, so prove it reaches every golden path.
        let covered: Vec<&str> = mutations.iter().map(|(p, _)| *p).collect();
        assert_eq!(
            covered,
            QsoEdit::project(&base).field_paths(),
            "a field of QsoEdit has no mutation here, so nothing proves the key can see it move"
        );

        for (path, mutate) in &mutations {
            let mut r = base.clone();
            mutate(&mut r);
            assert_ne!(
                QsoEdit::project(&r).key(),
                base_key,
                "editing {path} left the key unchanged, so a concurrent edit of it is invisible"
            );
        }
    }

    /// The point of a narrow key: the writes that happen on their own must not refuse an edit.
    #[test]
    fn edit_key_ignores_stamps_and_merges() {
        let base = record();
        let base_key = QsoEdit::project(&base).key();

        let mut stamped = base.clone();
        stamped.upload.lotw = Some(crate::logbook::UploadStatus {
            outcome: crate::logbook::UploadOutcome::Accepted,
            when_unix: 1_758_000_300,
            detail: None,
        });
        assert_eq!(
            QsoEdit::project(&stamped).key(),
            base_key,
            "an upload stamp moved the key"
        );

        let mut confirmed = base.clone();
        confirmed.confirmed = true;
        confirmed.award_confirmed = true;
        confirmed.qsl_rcvd.lotw = true;
        assert_eq!(
            QsoEdit::project(&confirmed).key(),
            base_key,
            "a LoTW confirmation moved the key"
        );

        let mut credited = base.clone();
        credited.credit_granted = vec!["DXCC".into()];
        credited.credit_submitted = vec!["WAS".into()];
        assert_eq!(
            QsoEdit::project(&credited).key(),
            base_key,
            "award credit moved the key"
        );

        let mut identified = base.clone();
        identified.id = Some(crate::logbook::RecordId::Provisional {
            hash: 7,
            ordinal: 0,
        });
        assert_eq!(
            QsoEdit::project(&identified).key(),
            base_key,
            "adopting an id moved the key"
        );

        // ⚠️ The deliberate exception, stated as a test so it is a decision and not a surprise:
        // a merge that FILLS IN a blank identifying field does move the key. It is a change an
        // edit could overwrite, so the operator is shown the current row and retries.
        let mut merged = base.clone();
        merged.state = Some("IL".into());
        assert_ne!(
            QsoEdit::project(&merged).key(),
            base_key,
            "a merge filling a blank STATE must move the key"
        );
    }

    /// The key is a function of the projection alone — two records differing only outside it
    /// hash the same, and the same record hashes the same twice.
    #[test]
    fn the_key_is_stable_and_total() {
        let r = record();
        assert_eq!(QsoEdit::project(&r).key(), QsoEdit::project(&r).key());
        assert_eq!(
            QsoEdit::project(&r).key().len(),
            16,
            "FNV-1a-64 as 16 hex digits"
        );

        // A non-finite frequency keys rather than panicking, and is still told apart from a
        // real one — asserting the difference rather than merely calling it, because "it did
        // not panic" is true of a `key()` that silently dropped the field.
        let mut broken = r.clone();
        broken.freq_mhz = f64::NAN;
        assert_ne!(QsoEdit::project(&broken).key(), QsoEdit::project(&r).key());
    }

    /// The form's own vocabulary for the QSL-sent mark, which the projection has to speak
    /// exactly: a sent mark with no recorded method is "SENT", not "not sent".
    #[test]
    fn a_sent_mark_without_a_method_is_not_read_as_unsent() {
        let mut r = record();
        r.qsl_sent.sent = true;
        r.qsl_sent.via = None;
        assert_eq!(QsoEdit::project(&r).qsl_sent_via.as_deref(), Some("SENT"));
        assert_ne!(
            QsoEdit::project(&r).key(),
            QsoEdit::project(&record()).key()
        );
    }
}
