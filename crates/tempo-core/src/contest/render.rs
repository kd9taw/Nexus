//! ⭐ **The one function that renders a sent exchange, and it takes a ROW.**
//!
//! > No per-QSO emitter may obtain a sent exchange from anything but a row.
//!
//! That rule is enforced structurally rather than by a guard a future author has to
//! remember. There is no function anywhere that produces a sent exchange from a
//! session or from a log: [`ContestSession::my_exchange`](super::ContestSession) is a
//! bare `Vec<FieldValue>` with no `Display`, no `to_string` and no join helper, and
//! [`sent_exchange`] below takes `&LoggedQso`. Writing the defect therefore requires
//! hand-rolling a `format!` over a vector of structs — which is the moment it becomes
//! visible in review, instead of looking exactly like correct code.
//!
//! **The defect, so the rule reads as something rather than as style.** A log-level
//! sent exchange interpolated into a per-QSO loop relabels every row already logged
//! the moment a mobile changes county: `cabrillo()` wrote the log's class and section
//! on every QSO line, and two live interop emitters hoisted
//! `format!("{} {}", my_class, my_section)` out of their own row loops. Same shape,
//! three places, found one at a time. The signature below is why there is no fourth.
use super::spec::{ExchangeSpec, FieldValue, RoleSpec};
use crate::fieldday::LoggedQso;

/// The exchange this row SENT, in the send order of the role that sent it.
///
/// Values come from `row.tx` and from nowhere else. A slot the role sends but the row
/// does not carry yields an empty value **in position**, so a caller writing fixed
/// columns still writes the right number of them.
pub fn sent_exchange(row: &LoggedQso, spec: &ExchangeSpec) -> Vec<FieldValue> {
    role_for(row, spec)
        .sends
        .iter()
        .map(|key| {
            row.tx
                .iter()
                .find(|v| v.key == *key)
                .cloned()
                .unwrap_or(FieldValue {
                    key: spec.field(key).map(|f| f.key).unwrap_or(""),
                    raw: String::new(),
                    domain: None,
                })
        })
        .collect()
}

/// [`sent_exchange`] as the space-separated string an interop emitter puts on the
/// wire (N1MM's `<sent_exchange>`, the WSJT-X type-5 `exchange_sent`).
pub fn sent_exchange_string(row: &LoggedQso, spec: &ExchangeSpec) -> String {
    sent_exchange(row, spec)
        .iter()
        .map(|v| v.raw.as_str())
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The role a row was logged under. The row carries its own role id because crossing a
/// state line changes which role I am, and a row logged before the crossing must keep
/// the one it was worked under.
pub fn role_for(row: &LoggedQso, spec: &ExchangeSpec) -> &'static RoleSpec {
    spec.roles
        .iter()
        .find(|r| r.id == row.role)
        .or_else(|| spec.roles.first())
        .expect("an ExchangeSpec always declares at least one role")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::{field_day, ContestSession};
    use crate::fieldday::{FdEvent, FieldDayLog};

    fn fd_log() -> FieldDayLog {
        FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        )
    }

    #[test]
    fn a_row_renders_its_own_sent_exchange_in_send_order() {
        let mut log = fd_log();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 100));
        let spec = field_day(FdEvent::ArrlFd);
        assert_eq!(sent_exchange_string(&log.qsos()[0], spec), "3A WI");
        assert_eq!(
            sent_exchange(&log.qsos()[0], spec)
                .iter()
                .map(|v| v.key)
                .collect::<Vec<_>>(),
            vec!["CLASS", "SECTION"],
        );
    }

    /// The whole point: the session moving does not move a row already logged.
    #[test]
    fn moving_the_session_does_not_move_a_row_already_logged() {
        let mut log = fd_log();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 100));
        log.session.my_exchange[1].raw = "IL".into();
        assert!(log.log_mode_at("W1AW", "1D", "CT", "PH", 0, 200));
        let spec = field_day(FdEvent::ArrlFd);
        assert_eq!(sent_exchange_string(&log.qsos()[0], spec), "3A WI");
        assert_eq!(sent_exchange_string(&log.qsos()[1], spec), "3A IL");
    }

    /// ⭐ The §3.3 mechanism, as a guard rather than as a comment: no renderer for a
    /// SESSION-level sent exchange exists anywhere in this module tree.
    ///
    /// A source-text check, because the property is the ABSENCE of an item and Rust
    /// has no way to assert that a call would not compile without a `trybuild` harness
    /// this crate does not have. It is exact about what it forbids: a `Display`/
    /// `ToString` impl on the two types a sent exchange is made of, and any function
    /// whose name says it turns `my_exchange` into a string.
    #[test]
    fn no_renderer_reaches_a_session() {
        const SOURCES: &[(&str, &str)] = &[
            ("session.rs", include_str!("session.rs")),
            ("render.rs", include_str!("render.rs")),
            ("spec.rs", include_str!("spec.rs")),
            ("carrier.rs", include_str!("carrier.rs")),
            ("dupe.rs", include_str!("dupe.rs")),
            ("mod.rs", include_str!("mod.rs")),
        ];
        const FORBIDDEN: &[&str] = &[
            "impl std::fmt::Display for FieldValue",
            "impl std::fmt::Display for ContestSession",
            "impl Display for FieldValue",
            "impl Display for ContestSession",
            "fn my_exchange_string",
            "fn my_exch_string",
        ];
        for (name, src) in SOURCES {
            // Only the code, not this test's own list of what it forbids.
            let code = src.split("mod tests").next().unwrap_or(src);
            for needle in FORBIDDEN {
                assert!(
                    !code.contains(needle),
                    "{name} defines {needle:?} — a session-level sent-exchange renderer \
                     is the defect §3.3 exists to make unrepresentable"
                );
            }
        }
        // POSITIVE CONTROL — a check that found nothing is not a result. The same
        // matcher over text that DOES contain one must trip.
        let planted = "impl std::fmt::Display for FieldValue { }";
        assert!(
            FORBIDDEN.iter().any(|n| planted.contains(n)),
            "the matcher cannot see a renderer that is really there"
        );
    }
}
