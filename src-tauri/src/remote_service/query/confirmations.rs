//! Confirmation diagnostics for the hosted Awards view ("why isn't this QSO
//! confirmed, and what is the one fix"), computed by the same engine call the
//! desktop Awards view makes. Read only: no upload, push, sync, reconcile or
//! sign-in is reachable from here, and the report names no account or key.
//! The document is bounded to what the Awards panel renders.
use serde_json::Value;
use std::sync::TryLockError;
use std::time::{Duration, Instant};
use tempo_app::dto::DiagnosticsReportDto;

const LOG_ROWS: usize = 1_000_000;
/// The Awards panel lists the first fifty diagnoses.
const DIAGNOSES: usize = 50;
const REASONS: usize = 8;
const BUCKETS: usize = 64;
const ONE_AWAY: usize = 512;
const BANDS: usize = 64;
const TEXT_BYTES: usize = 1024;

pub(super) fn read_engine(engine: &crate::SharedEngine) -> Result<Value, &'static str> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let (report, log_count) = loop {
        match engine.try_lock() {
            Ok(e) => {
                let count = e.log_records().len();
                if count > LOG_ROWS {
                    return Err("applicationTooLarge");
                }
                // The desktop's get_confirmation_diagnostics holds the engine for this
                // same call. The publisher reuses a capture for ten seconds, so browser
                // refreshes cannot hold it more often than that.
                let report = e.confirmation_diagnostics(crate::now_unix(), |call| {
                    propagation::dxcc::resolve(call).map(|i| i.entity.to_string())
                });
                break (report, count);
            }
            Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(TryLockError::WouldBlock) => return Err("applicationBusy"),
        }
    };
    project(DiagnosticsReportDto::from(report), log_count)
}

fn text(s: &str) -> Result<(), &'static str> {
    if s.len() > TEXT_BYTES {
        Err("applicationTooLarge")
    } else {
        Ok(())
    }
}

/// The desktop report, cut to the rows the panel shows. Bucket QSO indices are
/// dropped: they only feed the desktop's upload buttons, which a browser never has,
/// and a large log would otherwise send one index per unconfirmed contact.
fn project(mut report: DiagnosticsReportDto, log_count: usize) -> Result<Value, &'static str> {
    report.diagnoses.truncate(DIAGNOSES);
    for d in &mut report.diagnoses {
        d.reasons.truncate(REASONS);
        text(&d.award)?;
        text(&d.status)?;
        for r in &d.reasons {
            let a = &r.action;
            for s in [&r.code, &r.confidence, &r.explanation, &a.kind]
                .into_iter()
                .chain(
                    [
                        &a.source,
                        &a.detail,
                        &a.field,
                        &a.found,
                        &a.expected,
                        &a.logged,
                        &a.suggested,
                        &a.call,
                    ]
                    .into_iter()
                    .flatten(),
                )
            {
                text(s)?;
            }
        }
    }
    report.buckets.truncate(BUCKETS);
    for b in &mut report.buckets {
        text(&b.kind)?;
        b.qso_indices.clear();
    }
    report.one_away.truncate(ONE_AWAY);
    for o in &report.one_away {
        text(&o.entity)?;
        if o.bands.len() > BANDS {
            return Err("applicationTooLarge");
        }
        for band in &o.bands {
            text(band)?;
        }
    }
    let mut value = serde_json::to_value(&report).map_err(|_| "applicationUnavailable")?;
    value["logCount"] = log_count.into();
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::super::{Publisher, Request};
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tempo_app::dto::{ActionDto, QsoDiagnosisDto, ReasonDto};
    const ID: &str = "10000000-0000-4000-8000-000000000001";
    fn request() -> Value {
        json!({ "requestId": ID, "collection": "confirmations", "cursor": null, "search": "", "unconfirmed": false, "after": null })
    }
    fn engine(records: usize) -> crate::SharedEngine {
        let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )));
        let data: String = (0..records)
            .map(|i| {
                let call = format!("K1C{i:03}");
                format!("<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20250101<TIME_ON:6>010000<EQSL_QSL_RCVD:1>Y<EOR>\n", call.len())
            })
            .collect();
        engine.lock().unwrap().import_adif(&data);
        engine
    }
    fn desktop(engine: &crate::SharedEngine) -> DiagnosticsReportDto {
        let e = engine.lock().unwrap();
        DiagnosticsReportDto::from(e.confirmation_diagnostics(crate::now_unix(), |call| {
            propagation::dxcc::resolve(call).map(|i| i.entity.to_string())
        }))
    }

    #[test]
    fn diagnostics_are_an_argument_free_read() {
        assert!(serde_json::from_value::<Request>(request())
            .unwrap()
            .valid());
        for (field, bad) in [
            ("search", json!("K1C000")),
            ("unconfirmed", json!(true)),
            ("after", json!(1)),
            ("cursor", json!(format!("{ID}:1"))),
        ] {
            let mut value = request();
            value[field] = bad;
            assert!(!serde_json::from_value::<Request>(value).unwrap().valid());
        }
    }

    #[test]
    fn the_report_is_the_desktop_report_cut_to_the_panel() {
        let engine = engine(60);
        let desktop = desktop(&engine);
        assert!(
            desktop.diagnoses.len() > DIAGNOSES,
            "the fixture must exceed what the panel lists"
        );
        let before = engine.lock().unwrap().get_log();
        let value = read_engine(&engine).unwrap();
        assert_eq!(value["logCount"], 60);
        assert_eq!(
            value["diagnoses"],
            serde_json::to_value(&desktop.diagnoses[..DIAGNOSES]).unwrap()
        );
        assert_eq!(value["waitingOnPartner"], desktop.waiting_on_partner);
        assert_eq!(value["pendingLag"], desktop.pending_lag);
        assert_eq!(
            value["oneAway"],
            serde_json::to_value(&desktop.one_away).unwrap()
        );
        let buckets = value["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), desktop.buckets.len());
        assert!(!buckets.is_empty());
        for (row, native) in buckets.iter().zip(&desktop.buckets) {
            assert_eq!(
                row,
                &json!({ "kind": native.kind, "count": native.count, "qsoIndices": [] })
            );
        }
        assert_eq!(
            engine.lock().unwrap().get_log(),
            before,
            "diagnosing cannot change records or upload state"
        );
    }

    #[test]
    fn an_oversized_explanation_is_refused_rather_than_cut() {
        let action = ActionDto {
            kind: "uploadToLotw".into(),
            source: None,
            detail: None,
            field: None,
            found: None,
            expected: None,
            logged: None,
            suggested: None,
            call: None,
            other_index: None,
            until_unix: None,
        };
        let report = |explanation: String| DiagnosticsReportDto {
            diagnoses: vec![QsoDiagnosisDto {
                index: 0,
                award: "DXCC/WAS".into(),
                status: "needsAction".into(),
                reasons: vec![ReasonDto {
                    code: "r3".into(),
                    confidence: "confident".into(),
                    explanation,
                    action: action.clone(),
                }],
            }],
            buckets: Vec::new(),
            one_away: Vec::new(),
            waiting_on_partner: 0,
            pending_lag: 0,
        };
        assert!(project(report("x".repeat(TEXT_BYTES)), 1).is_ok());
        assert_eq!(
            project(report("x".repeat(TEXT_BYTES + 1)), 1),
            Err("applicationTooLarge")
        );
    }

    #[test]
    fn a_refresh_inside_ten_seconds_reuses_the_capture_and_a_busy_engine_is_refused() {
        let engine = engine(2);
        let query: Request = serde_json::from_value(request()).unwrap();
        let mut publisher = Publisher::default();
        let now = Instant::now();
        let page = |p: &mut Publisher, at: Instant| -> Value {
            serde_json::from_str(&p.read(&query, &engine, None, at).unwrap()).unwrap()
        };
        let first = page(&mut publisher, now);
        assert_eq!(first["meta"]["source"]["logCount"], 2);
        assert_eq!(
            page(&mut publisher, now + Duration::from_secs(9))["snapshotId"],
            first["snapshotId"]
        );
        assert_ne!(
            page(&mut publisher, now + Duration::from_secs(11))["snapshotId"],
            first["snapshotId"]
        );
        let _held = engine.lock().unwrap();
        assert_eq!(read_engine(&engine), Err("applicationBusy"));
    }
}
