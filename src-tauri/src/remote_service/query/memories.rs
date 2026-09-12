//! Ephemeral, main-WebView-owned channel observation. No persistence, migration,
//! file access or radio action. Only a closed subset of the bank may leave here.
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_BYTES: usize = 128 * 1024;
pub const TTL: Duration = Duration::from_secs(60);
pub type Bank = Arc<Mutex<Option<(Instant, Arc<Value>)>>>;
fn object(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}
fn text(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.len() <= 1024)
}
fn named(v: &Value) -> bool {
    text(v) && v.as_str().is_some_and(|s| !s.trim().is_empty())
}
fn number(v: &Value) -> bool {
    v.as_f64().is_some_and(f64::is_finite)
}
fn positive(v: &Value) -> bool {
    v.as_f64().is_some_and(|n| n.is_finite() && n > 0.0)
}
fn keys(v: &Map<String, Value>, required: &[&str], optional: &[&str]) -> bool {
    required.iter().all(|k| v.contains_key(*k))
        && v.keys()
            .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str()))
}
fn list(v: &Value, max: usize, check: impl Fn(&Value) -> bool) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() <= max && a.iter().all(check))
}
fn optional(v: &Map<String, Value>, key: &str, check: impl Fn(&Value) -> bool) -> bool {
    v.get(key).is_none_or(check)
}
fn one_of(v: &Value, values: &[&str]) -> bool {
    v.as_str().is_some_and(|s| values.contains(&s))
}
fn range(v: &Value, min: f64, max: f64) -> bool {
    v.as_f64().is_some_and(|n| n >= min && n <= max)
}
fn net(v: &Value) -> bool {
    let Some(v) = object(v) else { return false };
    keys(
        v,
        &["days", "utcTime", "alertEnabled", "alertLeadMin"],
        &["netControl", "description", "netloggerName"],
    ) && list(&v["days"], 7, |d| range(d, 0.0, 6.0))
        && v["utcTime"].as_str().is_some_and(|s| {
            let Some((h, m)) = s.split_once(':') else {
                return false;
            };
            (1..=2).contains(&h.len())
                && m.len() == 2
                && h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit())
        })
        && v["alertEnabled"].is_boolean()
        && positive(&v["alertLeadMin"])
        && ["netControl", "description", "netloggerName"]
            .iter()
            .all(|k| optional(v, k, text))
}
fn memory(v: &Value) -> bool {
    let Some(v) = object(v) else { return false };
    keys(
        v,
        &[
            "id", "name", "kind", "rxMhz", "mode", "groups", "favorite", "source",
        ],
        &[
            "offsetDir",
            "offsetMhz",
            "txMhz",
            "toneMode",
            "ctcssEncHz",
            "ctcssDecHz",
            "dtcsCode",
            "dtcsRxCode",
            "dtcsPol",
            "notes",
            "callsign",
            "grid",
            "lat",
            "lon",
            "skip",
            "lastUsedUtc",
            "net",
        ],
    ) && ["id", "name", "mode", "source"]
        .iter()
        .all(|k| named(&v[*k]))
        && positive(&v["rxMhz"])
        && v["favorite"].is_boolean()
        && one_of(
            &v["kind"],
            &[
                "repeater",
                "simplex",
                "hfnet",
                "calling",
                "pota",
                "digital",
                "satellite",
                "emcomm",
                "reference",
                "other",
            ],
        )
        && list(&v["groups"], 64, named)
        && optional(v, "offsetDir", |x| {
            one_of(x, &["simplex", "plus", "minus", "split"])
        })
        && optional(v, "toneMode", |x| {
            one_of(x, &["none", "tone", "tsql", "dtcs", "cross"])
        })
        && [
            "offsetMhz",
            "txMhz",
            "ctcssEncHz",
            "ctcssDecHz",
            "dtcsCode",
            "dtcsRxCode",
            "lastUsedUtc",
        ]
        .iter()
        .all(|k| optional(v, k, positive))
        && ["dtcsPol", "notes", "callsign", "grid"]
            .iter()
            .all(|k| optional(v, k, text))
        && optional(v, "lat", |x| range(x, -90.0, 90.0))
        && optional(v, "lon", |x| range(x, -180.0, 180.0))
        && optional(v, "skip", Value::is_boolean)
        && optional(v, "net", net)
}
fn group(v: &Value) -> bool {
    object(v).is_some_and(|v| {
        keys(v, &["id", "name", "order"], &[])
            && named(&v["id"])
            && named(&v["name"])
            && number(&v["order"])
    })
}
fn unique(v: &Value) -> bool {
    let Some(rows) = v.as_array() else {
        return false;
    };
    rows.iter().map(|r| &r["id"]).collect::<HashSet<_>>().len() == rows.len()
}
pub fn parse(raw: &str) -> Option<Value> {
    if raw.len() > MAX_BYTES {
        return None;
    }
    let v: Value = serde_json::from_str(raw).ok()?;
    let o = object(&v)?;
    (keys(o, &["version", "memories", "groups"], &[])
        && o["version"] == 2
        && list(&o["memories"], 512, memory)
        && list(&o["groups"], 64, group)
        && unique(&o["memories"])
        && unique(&o["groups"]))
    .then_some(v)
}
pub fn read(bank: &Bank) -> Result<Value, &'static str> {
    let (at, value) = bank
        .try_lock()
        .map_err(|_| "applicationBusy")?
        .clone()
        .ok_or("applicationUnavailable")?;
    let age = at.elapsed();
    if age >= TTL {
        return Err("applicationUnavailable");
    }
    Ok(json!({ "bank": &*value, "sourceAgeMs": age.as_millis() as u64 }))
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = include_str!("../../../../ui/src/remote-web/__fixtures__/memories.json");
    #[test]
    fn exact_bank_cold_busy_expired_and_recovery() {
        let bank = Bank::default();
        assert_eq!(read(&bank).unwrap_err(), "applicationUnavailable");
        let value = parse(FIXTURE).unwrap();
        *bank.lock().unwrap() = Some((Instant::now(), Arc::new(value.clone())));
        assert_eq!(read(&bank).unwrap()["bank"], value);
        let guard = bank.lock().unwrap();
        assert_eq!(read(&bank).unwrap_err(), "applicationBusy");
        drop(guard);
        bank.lock().unwrap().as_mut().unwrap().0 = Instant::now() - TTL;
        assert_eq!(read(&bank).unwrap_err(), "applicationUnavailable");
        bank.lock().unwrap().as_mut().unwrap().0 = Instant::now();
        assert!(read(&bank).is_ok());
    }
    #[test]
    fn closed_bank_bounds_preserve_free_modes_and_odd_split_fields() {
        let value = parse(FIXTURE).unwrap();
        for patch in [
            json!({"password":"no"}),
            json!({"version":1}),
            json!({"memories":[{}]}),
            json!({"groups":[] , "extra":true}),
        ] {
            let mut v = value.clone();
            v.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            assert!(parse(&v.to_string()).is_none());
        }
        let mut v = value.clone();
        v["memories"][0]["notes"] = json!("a".repeat(1025));
        assert!(parse(&v.to_string()).is_none());
        v = value.clone();
        v["memories"] = json!(vec![&value["memories"][0]; 513]);
        assert!(parse(&v.to_string()).is_none());
        v = value.clone();
        v["memories"][1]["id"] = v["memories"][0]["id"].clone();
        assert!(parse(&v.to_string()).is_none());
        assert!(parse(&" ".repeat(MAX_BYTES + 1)).is_none());
        assert!(parse(r#"{"version":2,"memories":[],"groups":[]}"#).is_some());
    }
    #[test]
    fn complete_count_and_byte_limits_have_positive_controls() {
        let rows: Vec<_> = (0..513).map(|n| json!({"id": format!("m{n}"), "name":"Calling", "kind":"calling", "rxMhz":14.06, "mode":"CW", "groups":[], "favorite":false, "source":"user"})).collect();
        assert!(
            parse(&json!({"version":2, "groups":[], "memories":&rows[..512]}).to_string())
                .is_some()
        );
        assert!(parse(&json!({"version":2, "groups":[], "memories":rows}).to_string()).is_none());
        let rich: Vec<_> = rows[..128]
            .iter()
            .cloned()
            .map(|mut r| {
                r["notes"] = json!("x".repeat(1024));
                r
            })
            .collect();
        assert!(
            parse(&json!({"version":2, "groups":[], "memories":&rich[..64]}).to_string()).is_some()
        );
        assert!(parse(&json!({"version":2, "groups":[], "memories":rich}).to_string()).is_none());
    }
}
