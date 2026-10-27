//! The Remote FT permit's slot on the Engine (`Engine::remote_transmit`), and the poll the radio
//! loop and FT planning and commit run on every tick. The verbs that set the permit went with the
//! old Remote page, so the slot is only ever cleared now; the poll's live work is a streamed
//! session's own hold on the transmitter (`remote_stream.rs`).
use super::Engine;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Installed once when Remote is built ([`Engine::set_remote_tx_hold_source`]): asks the service
/// whether the device driving the station from afar is held from transmit at `now`, re-derived from
/// the live lease and its recorded provenance (the 2026-10-23 sign-in decision). `None` while the
/// authority is momentarily busy (a request in flight is setting the gate itself) — the radio loop
/// then leaves the hold as that request set it.
pub type RemoteTxHoldSource = Arc<dyn Fn(Instant) -> Option<RemoteTxHold> + Send + Sync>;

/// What [`RemoteTxHoldSource`] reports of the device driving the station from afar at one instant.
/// `held` is the transmit gate's answer and the only part anything refuses on; the rest is for the
/// snapshot's `radio.txHold` line and never decides a transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemoteTxHold {
    /// Whether the device is held from transmit: what the gate is set to.
    pub held: bool,
    /// Whether the station holds a confirmed owner notice for the device. Display only.
    pub owner_notified: bool,
    /// What is left of the device's transmit window, when the window alone holds it and the owner
    /// was notified: the hold then lifts by itself at its end. `None` otherwise. Display only.
    pub left: Option<Duration>,
}

impl Engine {
    /// Install the source the radio loop re-derives the sign-in transmit hold from on every
    /// [`Self::poll_remote_transmit`] tick. See [`RemoteTxHoldSource`].
    pub fn set_remote_tx_hold_source(&mut self, source: RemoteTxHoldSource) {
        self.remote_tx_hold_source = Some(source);
    }

    pub fn remote_ft_tx_owned(&self) -> bool {
        self.tx_enabled()
            && self
                .remote_transmit
                .as_ref()
                .is_some_and(|p| p.valid(Instant::now()))
    }

    /// Called by the real radio loop and at both FT planning and commit. A
    /// disconnected host cannot leave the station waiting for another request
    /// to notice expiry. Native halt supplies the existing flush/unkey signals.
    /// A streamed session's presence and held PTT are polled here too, on the
    /// same tick (`remote_stream.rs`).
    pub fn poll_remote_transmit(&mut self, now: Instant) -> bool {
        // Re-derive the sign-in transmit hold from the live lease (the 2026-10-23 sign-in
        // decision). Done on every tick so the hold tracks the CURRENT controller: it lifts the
        // instant a held controller stops controlling by ANY path — Remote turned off, a
        // disconnect, a retired or expired lease — and a still-controlling held device stays held.
        // `None` means the authority was busy (a request in flight is setting the gate itself), so
        // the flag is left exactly as that request set it, never guessed.
        // The display copy (`radio.txHold`) is taken from the same answer, after the gate is set;
        // the gate never reads it back.
        if let Some(source) = self.remote_tx_hold_source.clone() {
            if let Some(hold) = source(now) {
                self.set_remote_transmit_held(hold.held);
                self.remote_tx_hold_shown = hold.held.then(|| crate::dto::TxHold {
                    owner_notified: hold.owner_notified,
                    left_secs: hold
                        .left
                        .filter(|_| hold.owner_notified)
                        .map(|left| left.as_secs() + u64::from(left.subsec_nanos() > 0)),
                });
            }
        }
        let halted = if self.remote_transmit.as_ref().is_some_and(|p| !p.valid(now)) {
            self.halt_tx();
            true
        } else {
            false
        };
        self.poll_remote_stream(now) || halted
    }
}
