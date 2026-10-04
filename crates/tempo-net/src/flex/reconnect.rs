//! The reconnect ladder: what to do when a session ends, in order.
//!
//! - **Unkey first.** If the session ended while one of our transmissions was unconfirmed, the
//!   first step is always [`Step::UnkeyLocally`]: stop feeding transmit audio and treat the radio
//!   as keyed until a new session's interlock says otherwise. Nothing else happens before it, so a
//!   connection is never swapped in under a keyed transmitter.
//! - **Retry on a ladder** ([`LADDER_MS`]: 1 s, 2 s, 5 s, 10 s, then every 30 s) after a lost
//!   connection, a keepalive loss, a protocol error, or an unkey the radio did not confirm. A
//!   session that registers resets the ladder.
//! - **Wait for the operator** after the radio refuses registration (a full radio is not fixed by
//!   asking again), and do nothing after an intentional close.
//! - **Remember our handles.** A radio can keep transmitting under a handle whose connection is
//!   gone (whether a disconnect unkeys a client is not established; port plan §3.4). The ladder
//!   keeps the handles of recent sessions, and the next session is given them, so it can recognise
//!   "our previous session still holds the transmitter", report it, and send its unkey, instead
//!   of taking it for another client.
//!
//! Pure: the steps are values and the caller performs them in order. No step can be a keying
//! command; the type has none.
//!
//! Nexus's own design, not a port. AetherSDR retries on a flat 5-second timer from its model.

use std::collections::VecDeque;

/// The waits between attempts, in order; the last repeats.
pub const LADDER_MS: [u64; 5] = [1_000, 2_000, 5_000, 10_000, 30_000];

/// How many of our past handles are remembered.
pub const REMEMBERED_HANDLES: usize = 4;

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The connection failed or the radio closed it.
    Lost,
    /// Too many ping replies were missed.
    KeepaliveLost,
    /// The radio did not confirm an unkey in time; the session sent its unkey again and closed.
    UnkeyUnconfirmed,
    /// The radio broke the protocol (a line too long to be one, a prologue that never came).
    ProtocolError,
    /// The radio refused `client gui`.
    RegistrationRejected { code: u32, detail: String },
    /// Closed on purpose.
    Closed,
}

/// Why the ladder stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    RegistrationRejected { code: u32, detail: String },
}

/// One thing to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Stop transmit audio and treat the radio as keyed until a session proves otherwise.
    UnkeyLocally,
    /// Connect again after this long.
    Retry { after_ms: u64 },
    /// Do not reconnect until the operator asks.
    WaitForOperator(Hold),
}

/// The ladder for one radio.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ladder {
    failures: usize,
    handles: VecDeque<u32>,
}

impl Ladder {
    pub fn new() -> Ladder {
        Ladder::default()
    }

    /// The session ended. `was_keyed`: a transmission of ours was unconfirmed. `handle`: the
    /// session's client handle, if it had one.
    pub fn ended(&mut self, end: &End, was_keyed: bool, handle: Option<u32>) -> Vec<Step> {
        if let Some(h) = handle.filter(|h| *h != 0) {
            self.handles.retain(|x| *x != h);
            self.handles.push_front(h);
            self.handles.truncate(REMEMBERED_HANDLES);
        }
        let mut steps = Vec::new();
        if was_keyed {
            steps.push(Step::UnkeyLocally);
        }
        match end {
            End::Closed => {}
            End::RegistrationRejected { code, detail } => {
                steps.push(Step::WaitForOperator(Hold::RegistrationRejected {
                    code: *code,
                    detail: detail.clone(),
                }));
            }
            End::Lost | End::KeepaliveLost | End::UnkeyUnconfirmed | End::ProtocolError => {
                let wait = LADDER_MS[self.failures.min(LADDER_MS.len() - 1)];
                self.failures += 1;
                steps.push(Step::Retry { after_ms: wait });
            }
        }
        steps
    }

    /// A session registered and subscribed: the next loss starts the ladder from the bottom.
    pub fn registered(&mut self) {
        self.failures = 0;
    }

    /// The operator asked to connect: a hold is lifted and the ladder starts again.
    pub fn operator_connect(&mut self) {
        self.failures = 0;
    }

    /// Our recent handles, newest first, for the next session.
    pub fn previous_handles(&self) -> Vec<u32> {
        self.handles.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyed_session_unkeys_before_anything_else() {
        let mut ladder = Ladder::new();
        for end in [
            End::Lost,
            End::KeepaliveLost,
            End::UnkeyUnconfirmed,
            End::ProtocolError,
            End::Closed,
            End::RegistrationRejected {
                code: 0xF300_0001,
                detail: String::new(),
            },
        ] {
            let steps = ladder.ended(&end, true, Some(1));
            assert_eq!(steps.first(), Some(&Step::UnkeyLocally), "{end:?}");
            assert_eq!(
                steps.iter().filter(|s| **s == Step::UnkeyLocally).count(),
                1,
                "{end:?}"
            );
        }
        // Unkeyed sessions have nothing to unkey.
        assert!(!ladder
            .ended(&End::Lost, false, None)
            .contains(&Step::UnkeyLocally));
    }

    #[test]
    fn the_ladder_climbs_caps_and_resets() {
        let mut ladder = Ladder::new();
        let waits: Vec<u64> = (0..7)
            .map(|_| match ladder.ended(&End::Lost, false, None).as_slice() {
                [Step::Retry { after_ms }] => *after_ms,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(waits, [1_000, 2_000, 5_000, 10_000, 30_000, 30_000, 30_000]);
        ladder.registered();
        assert_eq!(
            ladder.ended(&End::KeepaliveLost, false, None),
            [Step::Retry { after_ms: 1_000 }]
        );
    }

    #[test]
    fn a_refused_registration_waits_for_the_operator_and_a_close_stops() {
        let mut ladder = Ladder::new();
        let end = End::RegistrationRejected {
            code: 0xF300_0001,
            detail: "full".into(),
        };
        assert_eq!(
            ladder.ended(&end, false, Some(7)),
            [Step::WaitForOperator(Hold::RegistrationRejected {
                code: 0xF300_0001,
                detail: "full".into()
            })]
        );
        assert!(ladder.ended(&End::Closed, false, Some(8)).is_empty());
    }

    #[test]
    fn recent_handles_are_remembered_newest_first() {
        let mut ladder = Ladder::new();
        for h in [1, 2, 3, 2, 4, 5, 0] {
            ladder.ended(&End::Lost, false, Some(h));
        }
        assert_eq!(ladder.previous_handles(), [5, 4, 2, 3]);
    }
}
