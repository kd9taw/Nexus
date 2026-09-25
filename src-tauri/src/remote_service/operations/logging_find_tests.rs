//! A change's contact found in the store with the Engine lock released (`find`, then `locate`
//! under the lock), held to the code before SPEC-2 v3 C18 moved it: C16's `locate`, its body
//! VERBATIM (`old_locate`), over the log in memory, beside the store that log mirrors — and on the
//! 1.13 path. Every contact's own row, near misses and id targets, on logs with ties in call and
//! time, identical twins and calls outside ASCII, and after every one of the app's kinds of change.
use super::*;
use crate::remote_service::query::log_tests::{
    launch, random_change, settle, synthetic, test_country, test_state, Dir, Gen, MY_CALL,
};
use crate::remote_service::stored_log_tests::StoredLog;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tempo_core::logbook::adif_record_own_log;

// ── the code before C18, VERBATIM ────────────────────────────────────────────────────────────

/// The id of the contact a writer saw — the browser's log page or the shack's log view — or
/// `None` when no contact is that row any more: none holds exactly that content (a key
/// target), or the contact is gone or no longer the version the writer held (an id target).
/// A position is never the answer: one kept from an earlier read is stale the moment the
/// OTHER writer deletes above it, and acting on it deleted or rewrote a different contact.
/// Another instance's changes are folded in first, so the answer is about the log as it is.
///
/// A key target is found in the log as the store holds it ([`StoredLog`]), in place of the copy
/// in memory: these tests hold the old search against the new over the same rows, and whether
/// the store holds what the old write path wrote (P6) is the Stage-1 lockstep suite's job.
fn old_locate(engine: &mut Engine, target: &Target) -> Option<RecordId> {
    engine.take_in_shared_log();
    match target {
        Target::Key(t) => engine
            .stored_log()
            .into_iter()
            .find(|r| r.call == t.call && r.when_unix == t.when_unix && row_key(r) == t.key)
            .and_then(|r| r.id),
        Target::Id(t) => {
            let id = t.id.parse().ok()?;
            engine.fresh_log_row(id, &t.edit_key).ok().map(|_| id)
        }
    }
}

/// The new code's answer, as the change path gets it: the log's rows under the Engine lock, the
/// search with it released, and the found version still the contact's when the change is made
/// (the check `change_found` makes, asked of the store here).
fn new_locate(e: &crate::SharedEngine, target: &Target) -> Option<RecordId> {
    let rows = e.lock().unwrap().log_rows();
    let found = find(&rows, target).expect("the log reads")?;
    let id = found.id.parse().ok()?;
    let view = e.lock().unwrap().log_view();
    view.row(id)
        .expect("the log reads")
        .filter(|row| tempo_app::station::StationCore::fresh_row(row, &found.edit_key).is_ok())
        .map(|_| id)
}

// ── logs with ties and twins ─────────────────────────────────────────────────────────────────

/// `n` synthetic contacts, an eighth of them copies of one before — identical twins in every
/// field the file carries, each given its own id when the file is read — and many more sharing a
/// call and a second: times are drawn from a range a quarter the size of the log.
fn twins_log(n: usize, seed: u64) -> String {
    let mut g = Gen(seed | 1);
    let mut s = tempo_core::logbook::adif_header();
    let mut made: Vec<String> = Vec::new();
    for _ in 0..n {
        let one = if !made.is_empty() && g.chance(8) {
            made[g.below(made.len())].clone()
        } else {
            let when = 1_700_000_000 + 60 * g.below((n / 4).max(1)) as u64;
            synthetic(&mut g, when)
        };
        s.push_str(&one);
        made.push(one);
    }
    s
}

/// The same file on the 1.13 path, read as a launch reads it: an import would fold the twins.
fn loaded(d: &Dir) -> crate::SharedEngine {
    let mut e = Engine::new(MY_CALL, "EN52", 0);
    e.set_dxcc_resolver(test_country);
    e.set_state_resolver(test_state);
    e.set_log_path(d.log());
    assert!(e.log_on_file(), "premise: the 1.13 path");
    Arc::new(Mutex::new(e))
}

/// Every target a writer can send for this log: each contact's own row as a key target, as the
/// page and the shack build it, and its id target; and near misses — for every seventh contact
/// the row a second later, the call in lower case, a key no row has, and an edit key no version
/// has.
fn targets(e: &crate::SharedEngine) -> Vec<Target> {
    let eng = e.lock().unwrap();
    let key = |call: &str, when_unix: u64, key: String| {
        Target::Key(KeyTarget {
            call: call.to_string(),
            when_unix,
            key,
        })
    };
    let mut out = Vec::new();
    for (i, r) in eng.stored_log().iter().enumerate() {
        out.push(key(&r.call, r.when_unix, row_key(r)));
        let id =
            r.id.expect("every row the log holds carries an id")
                .to_string();
        out.push(Target::Id(RowRef {
            id: id.clone(),
            edit_key: QsoEdit::project(r).key(),
        }));
        if i % 7 == 0 {
            out.push(key(&r.call, r.when_unix + 1, row_key(r)));
            if r.call.to_lowercase() != r.call {
                out.push(key(&r.call.to_lowercase(), r.when_unix, row_key(r)));
            }
            out.push(key(&r.call, r.when_unix, "0".repeat(64)));
            out.push(Target::Id(RowRef {
                id,
                edit_key: "0".repeat(16),
            }));
        }
    }
    out
}

/// Every target of [`targets`], found by the new code and by the old, compared. How many were
/// found, and how many refused.
fn assert_found_as_before(e: &crate::SharedEngine, what: &str) -> (usize, usize) {
    let (mut found, mut refused) = (0, 0);
    for t in targets(e) {
        let new = new_locate(e, &t);
        let old = old_locate(&mut e.lock().unwrap(), &t);
        assert_eq!(new, old, "{what}: {}", serde_json::to_string(&t).unwrap());
        if new.is_some() {
            found += 1;
        } else {
            refused += 1;
        }
    }
    (found, refused)
}

/// ★ PARITY: over 2,000 contacts, on the store and on the 1.13 path, every target is found as the
/// old `locate` found it — every contact by its own row and by its id, and every near miss
/// refused. Premises first: the log really holds ties in call and time, identical twins and calls
/// outside ASCII.
#[test]
fn every_target_is_found_as_the_old_locate_found_it() {
    let text = twins_log(2_000, 0x0C18_A2F1);
    let (d, m) = (Dir::new("find-store"), Dir::new("find-memory"));
    std::fs::write(d.log(), &text).unwrap();
    std::fs::write(m.log(), &text).unwrap();
    let store = launch(&d);
    {
        let eng = store.lock().unwrap();
        let log = eng.stored_log();
        assert_eq!(log.len(), 2_000, "premise: every contact, twins too");
        let (mut seconds, mut copies) = (HashMap::new(), HashMap::new());
        for r in &log {
            *seconds.entry((r.call.clone(), r.when_unix)).or_insert(0) += 1;
            let mut unnamed = QsoRecord::clone(r);
            unnamed.id = None;
            *copies.entry(adif_record_own_log(&unnamed)).or_insert(0) += 1;
        }
        let shared = seconds.values().filter(|n| **n > 1).count();
        let twins = copies.values().filter(|n| **n > 1).count();
        assert!(shared > 50, "premise: calls sharing a second: {shared}");
        assert!(twins > 50, "premise: identical twins: {twins}");
        assert!(
            log.iter().any(|r| !r.call.is_ascii()),
            "premise: calls outside ASCII"
        );
    }
    for (arm, e) in [("the store", &store), ("the 1.13 path", &loaded(&m))] {
        let contacts = e.lock().unwrap().stored_log().len();
        let (found, refused) = assert_found_as_before(e, arm);
        assert_eq!(
            found,
            2 * contacts,
            "{arm}: every contact, by its row and by its id"
        );
        assert!(refused > 500, "{arm}: premise: near misses: {refused}");
    }
    settle(&store);
}

/// ★ THE PROPERTY: after every one of 16 changes to each of six seeded logs — edits that move a
/// contact in time or change its call, deletes, imports, marks, stamps, LoTW credit, the fill job
/// — every target is found in the store as the old `locate` found it in memory.
#[test]
fn after_every_change_every_target_is_found_as_before() {
    for seed in 1..=6u64 {
        let d = Dir::new(&format!("find-prop-{seed}"));
        std::fs::write(d.log(), twins_log(250, seed * 104_729)).unwrap();
        let e = launch(&d);
        let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for step in 0..16u64 {
            random_change(&e, &mut g, step);
            assert_found_as_before(&e, &format!("seed {seed}, step {step}"));
        }
        settle(&e);
    }
}

/// What was found is checked under the lock: an edit landing between the search and the lock
/// makes the contact another version, and it is refused, as an id target would be; an upload
/// stamp does not, since the edit key covers only what an edit writes. The control: nothing
/// landing, it is found.
#[test]
fn what_the_search_found_is_checked_again_under_the_lock() {
    let d = Dir::new("find-between");
    std::fs::write(d.log(), twins_log(60, 0x0C18_A2F2)).unwrap();
    let e = launch(&d);
    let first = |e: &crate::SharedEngine| {
        let eng = e.lock().unwrap();
        let r = QsoRecord::clone(&eng.stored_log()[0]);
        let t = Target::Key(KeyTarget {
            call: r.call.clone(),
            when_unix: r.when_unix,
            key: row_key(&r),
        });
        (r, t)
    };
    let search = |e: &crate::SharedEngine, t: &Target| {
        let rows = e.lock().unwrap().log_rows();
        find(&rows, t).expect("the log reads").expect("found")
    };
    let (r, t) = first(&e);
    let found = search(&e, &t);
    assert_eq!(locate(&e.lock().unwrap(), &found), r.id, "control");

    // A stamp lands between the two: the same version, so it stands.
    assert!(e.lock().unwrap().stamp_qrz_upload(
        &r,
        tempo_core::logbook::UploadOutcome::Accepted,
        1_789_000_000,
        None
    ));
    assert_eq!(locate(&e.lock().unwrap(), &found), r.id);

    // An edit lands between the two: another version, refused.
    let (r, t) = first(&e);
    let found = search(&e, &t);
    let mut edited = r.clone();
    edited.comment = Some("changed at the shack".into());
    assert!(e.lock().unwrap().update_qso(r.id.unwrap(), edited));
    assert_eq!(locate(&e.lock().unwrap(), &found), None);
    settle(&e);
}

/// ★ POSITIVE CONTROL for the fence the search of a key target passes: made while this thread
/// holds an Engine guard, it is a panic in a debug build.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "io_fence: a Remote read of the log is a pass over the whole log")]
fn a_key_target_searched_under_the_engine_lock_is_refused() {
    let d = Dir::new("find-fence");
    std::fs::write(d.log(), twins_log(5, 0x0C18_A2F3)).unwrap();
    let e = launch(&d);
    let r = QsoRecord::clone(&e.lock().unwrap().stored_log()[0]);
    let t = Target::Key(KeyTarget {
        call: r.call.clone(),
        when_unix: r.when_unix,
        key: row_key(&r),
    });
    let eng = tempo_app::engine::engine_lock(&e);
    let rows = eng.log_rows();
    let _ = find(&rows, &t);
}
