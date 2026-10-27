//! One alert, as every screen reads it: made once, in Rust, where it is raised, and given an id of
//! its own there. The shack's own windows hear it as the [`EVENT`] event, and a computer driving
//! the shack hears the same event on its command link (`command_dispatch::FORWARDED_EVENTS`), with
//! the same id, so every screen, and whatever reads an alert later, names the same one.
//!
//! The shape is versioned ([`VERSION`]): `v`, `id`, `kind` (what raised it: `pounce`, `needed` for
//! a heard station worth interrupting for, `parks` for parks newly on the air),
//! `atMs` (when, by the shack's clock) and `alert` (that kind's own payload, unchanged). A reader
//! takes the kinds and the version it knows, and leaves the rest.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::Serialize;

/// The event every alert is raised as, on each window and on the command link (the client core's,
/// so both ends name it alike).
pub(crate) use tempo_client::alert::EVENT;
/// The shape's version.
pub(crate) const VERSION: u32 = 1;

/// One alert raised at the shack.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Alert<T> {
    pub(crate) v: u32,
    pub(crate) id: u64,
    pub(crate) kind: &'static str,
    pub(crate) at_ms: u64,
    pub(crate) alert: T,
}

/// A new alert of `kind`, carrying `alert`, stamped now, with this run's next id. The ids rise
/// from a random start under 2^40, so one run's never read as another's, and each stays a whole
/// number a screen's JavaScript holds exactly.
pub(crate) fn raised<T>(kind: &'static str, alert: T) -> Alert<T> {
    static NEXT: OnceLock<AtomicU64> = OnceLock::new();
    let next = NEXT.get_or_init(|| {
        use ring::rand::SecureRandom;
        let mut bytes = [0u8; 8];
        let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
        AtomicU64::new((u64::from_be_bytes(bytes) >> 24).max(1))
    });
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default();
    Alert {
        v: VERSION,
        id: next.fetch_add(1, Ordering::Relaxed),
        kind,
        at_ms,
        alert,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// ★ An alert's id is made where it is raised: each new one takes the next, every id a whole
    /// number JavaScript holds exactly, and its shape is the versioned one a screen reads, the
    /// kind's own payload unchanged inside it. CONTROL: two alerts raised one after the other are
    /// two ids.
    #[test]
    fn an_alert_is_made_once_with_its_own_id_and_the_versioned_shape() {
        let first = raised("pounce", json!({ "call": "VP8PJ" }));
        let second = raised("pounce", json!({ "call": "3Y0J" }));
        assert_eq!(second.id, first.id + 1);
        assert!(first.id >= 1 && second.id < 1 << 53, "{}", second.id);
        let read = serde_json::to_value(&first).unwrap();
        assert_eq!(
            read,
            json!({ "v": 1, "id": first.id, "kind": "pounce", "atMs": first.at_ms,
                "alert": { "call": "VP8PJ" } })
        );
        assert!(
            first.at_ms > 1_700_000_000_000,
            "stamped by the clock: {}",
            first.at_ms
        );
    }
}
