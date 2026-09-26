//! Remote Sub-receiver levels: RF gain, AF gain and squelch on a dual-receiver radio's SECOND
//! receiver, from the hosted page.
//!
//! ONE engine verb serves both surfaces: admission ends in [`Engine::request_sub_level`], the verb
//! the desktop's Sub row calls, so every refusal the desktop gets is the page's too — no Sub
//! offered (UNKNOWN is never an offer, D3), a stage no vendor statement credits to the Sub (D7), a
//! CAT path that cannot name the Sub. Before it, the admission every receive-display one-shot takes
//! (the rig-scope settings are the precedent): the permit, a native source, a fresh unkeyed CAT
//! link to the radio the page displayed, and no owned transmitter or tune carrier. The radio loop
//! then applies the level at receive time exactly as for a local drag, and withholds it while the
//! rig is keyed.
//!
//! Nothing here arms, keys, retunes or saves Settings. The receipt says only that the station took
//! the request; what the RADIO accepted reaches the page in the snapshot (`receivers.sub`), and
//! nothing reads the Sub back, so no receipt may claim a read-back.
use super::*;
use crate::engine::sub_controls::{SubLevel, SubLevelRefusal};

impl Engine {
    pub fn queue_remote_sub_level(
        &mut self,
        level: SubLevel,
        value: f32,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        // A fresh, unkeyed CAT link to the radio the page displayed.
        self.remote_radio_link(connection)?;
        // Any owned transmitter — a tune carrier included (`TxOwner::Tune`).
        if self.tx_owner().is_some() {
            return Err(Reason::StationBusy);
        }
        // The grammar bounds the value; the station never clamps one the page should not have sent.
        if !(0.0..=1.0).contains(&value) {
            return Err(Reason::InvalidAction);
        }
        self.request_sub_level(level, value)
            .map_err(|refusal| match refusal {
                SubLevelRefusal::NotALevel => Reason::InvalidAction,
                SubLevelRefusal::NoSub
                | SubLevelRefusal::StageNotConfirmed(_)
                | SubLevelRefusal::NoRoute => Reason::HardwareUnavailable,
            })
    }
}
