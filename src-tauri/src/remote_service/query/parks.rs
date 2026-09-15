//! The station's own offline POTA park directory, searched by the hosted log
//! form (reference prefix or name fragment) and looked up by exact reference.
//! Only the in-memory index already loaded from the local cache is read: no
//! download, import, live POTA lookup or file access is reachable from here.
use serde_json::{json, Value};
use std::sync::TryLockError;

/// The most suggestions one search returns. The desktop form asks for eight.
const ROWS: usize = 12;
const REFERENCE_BYTES: usize = 32;
const GRID_BYTES: usize = 16;
const TEXT_BYTES: usize = 256;

/// A search is 2 to 32 bytes with no surrounding space. Control characters are
/// refused by the shared request check before this runs.
pub(super) fn valid_search(search: &str) -> bool {
    (2..=32).contains(&search.len()) && search.trim() == search
}

fn row(park: tempo_core::pota::Park) -> Option<Value> {
    let park = crate::ParkDto::from(park);
    (!park.reference.is_empty()
        && park.reference.len() <= REFERENCE_BYTES
        && park.grid.len() <= GRID_BYTES
        && park.name.len() <= TEXT_BYTES
        && park.location.len() <= TEXT_BYTES)
        .then(|| serde_json::to_value(park).ok())
        .flatten()
}

/// The same `search` and `lookup` the desktop commands run, on the same index.
pub(super) fn read(
    parks: &crate::SharedParks,
    search: &str,
) -> Result<(Vec<Value>, usize, Value), &'static str> {
    if !valid_search(search) {
        return Err("applicationUnsupported");
    }
    let (hits, exact, count) = match parks.try_lock() {
        Ok(index) => (
            index.search(search, ROWS),
            index.lookup(search),
            index.len(),
        ),
        Err(TryLockError::WouldBlock) => return Err("applicationBusy"),
        Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
    };
    // A malformed imported row that cannot fit the display contract is left out
    // rather than failing every search that happens to match it.
    let rows: Vec<Value> = hits.into_iter().filter_map(row).collect();
    let total = rows.len();
    Ok((
        rows,
        total,
        json!({ "parkCount": count, "exact": exact.and_then(row) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::{Publisher, Request};
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    const ID: &str = "10000000-0000-4000-8000-000000000001";
    const CSV: &str = "reference,name,grid,location\nUS-0001,Acadia National Park,FN54,US-ME\nUS-0010,Synthetic Lake,FN31,US-CT\nUS-0011,Another Lake,FN31,US-CT\n";
    fn index(csv: &str) -> crate::SharedParks {
        Arc::new(Mutex::new(tempo_core::pota::ParkIndex::parse_csv(csv)))
    }
    fn request(search: &str) -> Value {
        json!({ "requestId": ID, "collection": "parks", "cursor": null, "search": search, "unconfirmed": false, "after": null })
    }

    #[test]
    fn a_park_search_is_a_bounded_text_read_with_no_cursor_or_filter() {
        assert!(serde_json::from_value::<Request>(request("US-00"))
            .unwrap()
            .valid());
        let long = "X".repeat(33);
        for search in ["", "U", " US-0001", "US-0001 ", long.as_str(), "US\u{7}00"] {
            let query: Request = serde_json::from_value(request(search)).unwrap();
            assert!(!query.valid(), "{search:?} must be refused");
        }
        for (field, bad) in [
            ("unconfirmed", json!(true)),
            ("after", json!(1)),
            ("cursor", json!(format!("{ID}:1"))),
        ] {
            let mut value = request("US-00");
            value[field] = bad;
            assert!(!serde_json::from_value::<Request>(value).unwrap().valid());
        }
        // An unknown argument (a path, a live-lookup flag) is not part of the request at all.
        let mut value = request("US-00");
        value["live"] = json!(true);
        assert!(serde_json::from_value::<Request>(value).is_err());
    }

    #[test]
    fn search_matches_the_desktop_index_and_carries_the_exact_reference() {
        let parks = index(CSV);
        let (rows, total, meta) = read(&parks, "US-00").unwrap();
        let desktop: Vec<Value> = parks
            .lock()
            .unwrap()
            .search("US-00", ROWS)
            .into_iter()
            .map(|p| serde_json::to_value(crate::ParkDto::from(p)).unwrap())
            .collect();
        assert_eq!(rows, desktop);
        assert_eq!(total, 3);
        assert_eq!(meta, json!({ "parkCount": 3, "exact": null }));
        assert_eq!(read(&parks, "LAKE").unwrap().0.len(), 2);
        let (_, _, meta) = read(&parks, "US-0001").unwrap();
        assert_eq!(meta["exact"]["name"], "Acadia National Park");
        assert_eq!(meta["exact"]["latitude"], Value::Null);
        assert_eq!(meta["exact"]["longitude"], Value::Null);
    }

    #[test]
    fn an_empty_directory_a_busy_index_and_missing_sources_stay_distinct() {
        let empty: crate::SharedParks = Default::default();
        assert_eq!(
            read(&empty, "US-00").unwrap(),
            (Vec::new(), 0, json!({ "parkCount": 0, "exact": null }))
        );
        let parks = index(CSV);
        let held = parks.lock().unwrap();
        assert_eq!(read(&parks, "US-00"), Err("applicationBusy"));
        drop(held);
        let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )));
        let query: Request = serde_json::from_value(request("US-00")).unwrap();
        assert_eq!(
            Publisher::default().read(&query, &engine, None, Instant::now()),
            Err("applicationUnavailable")
        );
    }

    #[test]
    fn a_row_that_cannot_fit_the_display_contract_is_left_out_not_fatal() {
        let csv = format!("{CSV}US-0012,{} Lake,FN31,US-CT\n", "N".repeat(300));
        let parks = index(&csv);
        let (rows, total, _) = read(&parks, "US-00").unwrap();
        assert_eq!((rows.len(), total), (3, 3));
        assert!(rows.iter().all(|r| r["reference"] != "US-0012"));
    }
}
