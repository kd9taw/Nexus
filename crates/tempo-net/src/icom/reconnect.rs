//! What to do when an Icom network session ends: Nexus's reconnect policy.
//!
//! - **Retry on a ladder** ([`LADDER_MS`]: 1, 2, 4, 8 and 16 s, then every 30 s) after a link
//!   that went quiet or kept failing, or a connect the radio did not answer or refused as busy. A
//!   radio holds a lost session's slot for its own timeout (reported as about 20 s on some radios
//!   and up to 3 minutes on others), so the early attempts are expected to fail, and they are
//!   spaced out rather than repeated every tick: each attempt that fails part-way hands its token
//!   back and disconnects, and a radio walked away from refuses new attempts for longer. A session
//!   that connects starts the ladder again.
//! - **Wait for the operator** when asking again cannot help: the radio said another client has
//!   taken it (reconnecting would only take the radio back from the program the operator just
//!   started), the radio refused the user name or password, or the configuration names a radio or
//!   a rate the radio does not offer.
//! - **Stop** after the owner closed the session on purpose.
//!
//! Pure: the decisions are values and the caller acts on them. A session never reconnects by
//! itself. Nothing here keys: no session at this stage can transmit, so re-establishing one is
//! never done behind a keyed transmitter; transmit over the network will need its own rule here.
//!
//! Nexus's own design, not a port. Upstream's background reconnect (off by default) doubles
//! from 1 s to a 30 s cap; the delays here are the same, and the decision to retry is the
//! owner's, never the session's.

use super::session::{ConnectError, LossReason};

/// The waits before each attempt, in order; the last repeats.
pub const LADDER_MS: [u64; 6] = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000];

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// A working session was lost.
    Lost(LossReason),
    /// The connect failed.
    Failed(ConnectError),
    /// The owner closed it.
    Closed,
}

/// What to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Start a new session after this many milliseconds.
    RetryAfter(u64),
    /// Do nothing until the operator acts.
    WaitForOperator,
    /// Do nothing.
    Stop,
}

/// The ladder's position.
#[derive(Debug, Clone, Default)]
pub struct Ladder {
    step: usize,
}

impl Ladder {
    pub fn new() -> Ladder {
        Ladder::default()
    }

    /// What to do after `end`. A retry climbs the ladder.
    pub fn after(&mut self, end: &End) -> Next {
        let retry = match end {
            End::Closed => return Next::Stop,
            End::Lost(LossReason::PeerDisconnect) => false,
            End::Lost(LossReason::LinkTimeout | LossReason::SocketError) => true,
            End::Failed(error) => match error {
                ConnectError::NoAnswer(_) | ConnectError::Busy(_) | ConnectError::NoCivPort => true,
                ConnectError::LoginRefused
                | ConnectError::NoRadio(_)
                | ConnectError::RateNotOffered(_)
                | ConnectError::Request(_) => false,
            },
        };
        if !retry {
            return Next::WaitForOperator;
        }
        let wait = LADDER_MS[self.step.min(LADDER_MS.len() - 1)];
        self.step = (self.step + 1).min(LADDER_MS.len() - 1);
        Next::RetryAfter(wait)
    }

    /// A session connected: the next loss starts the ladder again.
    pub fn connected(&mut self) {
        self.step = 0;
    }
}

#[cfg(test)]
mod tests {
    //! Nexus's own rules. Upstream's reconnect cases are translated in the session's tests,
    //! against this policy.
    use super::*;
    use crate::icom::caps::Unselected;
    use crate::icom::wire::WireError;

    #[test]
    fn the_ladder_is_one_two_four_eight_sixteen_then_every_thirty_seconds() {
        let mut ladder = Ladder::new();
        let waits: Vec<Next> = (0..9)
            .map(|_| ladder.after(&End::Lost(LossReason::LinkTimeout)))
            .collect();
        let expected: Vec<Next> = [1, 2, 4, 8, 16, 30, 30, 30, 30]
            .iter()
            .map(|s| Next::RetryAfter(s * 1000))
            .collect();
        assert_eq!(waits, expected);
    }

    #[test]
    fn a_connected_session_starts_the_ladder_again() {
        let mut ladder = Ladder::new();
        for _ in 0..4 {
            ladder.after(&End::Failed(ConnectError::Busy(1)));
        }
        ladder.connected();
        assert_eq!(
            ladder.after(&End::Lost(LossReason::SocketError)),
            Next::RetryAfter(1000)
        );
    }

    #[test]
    fn what_retries_and_what_waits() {
        for end in [
            End::Lost(LossReason::LinkTimeout),
            End::Lost(LossReason::SocketError),
            End::Failed(ConnectError::NoAnswer(
                crate::icom::session::Stage::ControlProbe,
            )),
            End::Failed(ConnectError::Busy(0xffff_ffff)),
            End::Failed(ConnectError::NoCivPort),
        ] {
            assert!(
                matches!(Ladder::new().after(&end), Next::RetryAfter(1000)),
                "{end:?}"
            );
        }
        for end in [
            End::Lost(LossReason::PeerDisconnect),
            End::Failed(ConnectError::LoginRefused),
            End::Failed(ConnectError::NoRadio(Unselected::NoSuchName)),
            End::Failed(ConnectError::RateNotOffered(crate::icom::caps::Rates(
                0x0100,
            ))),
            End::Failed(ConnectError::Request(WireError::Field("credential"))),
        ] {
            assert_eq!(Ladder::new().after(&end), Next::WaitForOperator, "{end:?}");
        }
        assert_eq!(Ladder::new().after(&End::Closed), Next::Stop);
    }

    #[test]
    fn waiting_does_not_climb_the_ladder() {
        let mut ladder = Ladder::new();
        ladder.after(&End::Lost(LossReason::PeerDisconnect));
        ladder.after(&End::Closed);
        assert_eq!(
            ladder.after(&End::Lost(LossReason::LinkTimeout)),
            Next::RetryAfter(1000)
        );
    }
}
