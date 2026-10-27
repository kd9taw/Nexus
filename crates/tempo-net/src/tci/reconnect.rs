//! What to do when a TCI connection fails or a session ends: Nexus's reconnect policy.
//!
//! - **Our key first.** A session that ended with a key of ours possibly on the air hands that
//!   belief on ([`Step::StillKeyed`]): the engine halts its transmission and tells the operator the
//!   radio may still be transmitting, and the next session is given the belief
//!   ([`super::session::Config::carried`]) so its stops go out first after `ready;`.
//!   A lost link never clears it; only the server's own report does.
//! - **At once, then on a ladder.** A link lost while keyed is retried at once,
//!   once until a session reaches `ready;` again. Every other retry waits on the ladder
//!   ([`LADDER_MS`]: 1, 2, 5 and 10 s, then every 30 s), which starts again from the bottom
//!   whenever a session reaches `ready;`. An unkey the server did not confirm is retried on the
//!   ladder, never at once: the session already sent its stops twice and raised the alarm, and a
//!   server that holds the key (a stuck transmitter) is asked again on every retry.
//! - **Wait for the operator after repeated refusals** (the Icom network precedent): an address
//!   that answers but is not a TCI server (the WebSocket handshake fails), or a server that never
//!   says `ready;`, is asked [`REFUSALS_BEFORE_HOLD`] times and then left until the operator asks.
//!   An address where nothing answers is never held: the SDR program may simply not be running
//!   yet, and it is tried every 30 s once the ladder tops out.
//! - **Stop** after a close on purpose.
//!
//! Pure: the decisions are values and the caller acts on them; a session never reconnects by
//! itself. No step is a key: the type has none.
//!
//! Nexus's own design, not a port.

use super::session::{ConnectError, End as SessionEnd, OurKey};

/// The waits between attempts, in order; the last repeats.
pub const LADDER_MS: [u64; 5] = [1_000, 2_000, 5_000, 10_000, 30_000];

/// How many refusals in a row before the ladder waits for the operator.
pub const REFUSALS_BEFORE_HOLD: u32 = 3;

/// How a connection attempt or a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The connect failed ([`super::session::Connection::connect`]).
    Failed(ConnectError),
    /// A session ended ([`super::session::Event::Closed`]).
    Ended(SessionEnd),
}

/// Why the ladder stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    /// The address answered [`REFUSALS_BEFORE_HOLD`] times in a row without becoming a TCI
    /// session; the last reason.
    Refused(String),
}

/// One thing to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A key of ours may still be on the air: halt the transmission, raise the alarm, and give
    /// the next session this belief so its stops go first.
    StillKeyed(OurKey),
    /// Connect again after this long.
    Retry { after_ms: u64 },
    /// Do not reconnect until the operator asks.
    WaitForOperator(Hold),
}

/// The ladder for one radio.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ladder {
    failures: usize,
    refusals: u32,
    /// The one retry at once since the last `ready;` has been used.
    retried_at_once: bool,
}

impl Ladder {
    pub fn new() -> Ladder {
        Ladder::default()
    }

    /// A session reached `ready;`: the address speaks TCI, so the next loss starts the ladder from
    /// the bottom.
    pub fn ready(&mut self) {
        self.failures = 0;
        self.refusals = 0;
        self.retried_at_once = false;
    }

    /// The operator asked to connect: a hold is lifted and the ladder starts again.
    pub fn operator_connect(&mut self) {
        self.failures = 0;
        self.refusals = 0;
        self.retried_at_once = false;
    }

    /// What to do after `end`. `carried`: what the session left possibly on the air.
    pub fn ended(&mut self, end: &End, carried: Option<OurKey>) -> Vec<Step> {
        let mut steps = Vec::new();
        if let Some(carried) = carried {
            steps.push(Step::StillKeyed(carried));
        }
        let refusal = match end {
            End::Ended(SessionEnd::Closed) => return steps,
            End::Failed(ConnectError::Handshake(why)) => Some(why.clone()),
            End::Ended(SessionEnd::NoReady) => Some("the server never said it was ready".into()),
            End::Failed(ConnectError::Unreachable(_) | ConnectError::Spawn(_))
            | End::Ended(
                SessionEnd::Lost
                | SessionEnd::Stale
                | SessionEnd::ProtocolError(_)
                | SessionEnd::UnkeyUnconfirmed,
            ) => None,
        };
        if let Some(why) = refusal {
            self.refusals += 1;
            if self.refusals >= REFUSALS_BEFORE_HOLD {
                steps.push(Step::WaitForOperator(Hold::Refused(why)));
                return steps;
            }
        }
        let at_once = carried.is_some()
            && !self.retried_at_once
            && matches!(
                end,
                End::Ended(SessionEnd::Lost | SessionEnd::Stale | SessionEnd::ProtocolError(_))
            );
        if at_once {
            self.retried_at_once = true;
            steps.push(Step::Retry { after_ms: 0 });
            return steps;
        }
        let after_ms = LADDER_MS[self.failures.min(LADDER_MS.len() - 1)];
        self.failures += 1;
        steps.push(Step::Retry { after_ms });
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tci::admission::TxStart;
    use crate::tci::Trx;

    fn keyed() -> Option<OurKey> {
        Some(OurKey {
            trx: Trx::new(0).expect("trx 0"),
            kind: TxStart::Audio,
        })
    }

    fn waits(ladder: &mut Ladder, end: &End, n: usize) -> Vec<u64> {
        (0..n)
            .map(|_| match ladder.ended(end, None).as_slice() {
                [Step::Retry { after_ms }] => *after_ms,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn the_ladder_climbs_caps_and_starts_again_at_ready() {
        let mut ladder = Ladder::new();
        let lost = End::Ended(SessionEnd::Lost);
        assert_eq!(
            waits(&mut ladder, &lost, 7),
            [1_000, 2_000, 5_000, 10_000, 30_000, 30_000, 30_000]
        );
        ladder.ready();
        assert_eq!(waits(&mut ladder, &lost, 1), [1_000]);
        // Nothing listening: the SDR program may start later; never held.
        let mut ladder = Ladder::new();
        let unreachable = End::Failed(ConnectError::Unreachable("refused".into()));
        assert_eq!(waits(&mut ladder, &unreachable, 6)[5], 30_000);
    }

    #[test]
    fn a_link_lost_while_keyed_hands_the_belief_on_and_retries_at_once() {
        let mut ladder = Ladder::new();
        ladder.ready();
        for end in [
            SessionEnd::Lost,
            SessionEnd::Stale,
            SessionEnd::ProtocolError("too long".into()),
        ] {
            let mut l = ladder.clone();
            assert_eq!(
                l.ended(&End::Ended(end.clone()), keyed()),
                [
                    Step::StillKeyed(keyed().expect("keyed")),
                    Step::Retry { after_ms: 0 }
                ],
                "{end:?}"
            );
            // Only the first: a second loss before the next ready waits on the ladder.
            assert_eq!(
                l.ended(&End::Ended(end.clone()), keyed())[1],
                Step::Retry { after_ms: 1_000 },
                "{end:?}"
            );
        }
        // An unconfirmed unkey has already been escalated: the ladder, never at once.
        let mut l = ladder.clone();
        assert_eq!(
            l.ended(&End::Ended(SessionEnd::UnkeyUnconfirmed), keyed()),
            [
                Step::StillKeyed(keyed().expect("keyed")),
                Step::Retry { after_ms: 1_000 }
            ]
        );
        // A close on purpose while keyed: the belief goes on, nothing reconnects.
        assert_eq!(
            Ladder::new().ended(&End::Ended(SessionEnd::Closed), keyed()),
            [Step::StillKeyed(keyed().expect("keyed"))]
        );
        assert!(Ladder::new()
            .ended(&End::Ended(SessionEnd::Closed), None)
            .is_empty());
    }

    #[test]
    fn repeated_refusals_wait_for_the_operator_until_asked() {
        let mut ladder = Ladder::new();
        let refused = End::Failed(ConnectError::Handshake("HTTP 404".into()));
        assert_eq!(waits(&mut ladder, &refused, 2), [1_000, 2_000]);
        assert_eq!(
            ladder.ended(&refused, None),
            [Step::WaitForOperator(Hold::Refused("HTTP 404".into()))]
        );
        ladder.operator_connect();
        assert_eq!(waits(&mut ladder, &refused, 1), [1_000]);
        // A server that never says ready counts the same; a session that reached ready resets.
        let mut ladder = Ladder::new();
        let no_ready = End::Ended(SessionEnd::NoReady);
        waits(&mut ladder, &no_ready, 2);
        ladder.ready();
        assert_eq!(waits(&mut ladder, &no_ready, 2), [1_000, 2_000]);
        assert!(matches!(
            ladder.ended(&no_ready, None).as_slice(),
            [Step::WaitForOperator(_)]
        ));
        // A key still believed on the air is handed on even when the ladder holds.
        let mut ladder = Ladder::new();
        waits(&mut ladder, &no_ready, 2);
        assert_eq!(
            ladder.ended(&no_ready, keyed())[0],
            Step::StillKeyed(keyed().expect("keyed"))
        );
    }
}
