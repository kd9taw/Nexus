//! Test support for Nexus's native FlexRadio client: a SmartSDR-protocol simulator, and the
//! observe-only recorder testers run to capture a session from their radio.
//!
//! There is no Flex on the bench (testers only, maintainer ruling 2026-10-03), so the Flex client
//! is tested against [`Simulator`]: a SmartSDR TCP API server on loopback, with VITA-49 over UDP.
//! It replays a [`Session`], either synthetic (written by hand from the protocol authority) or
//! recorded from a real radio by [`record::record`], and answers each command by the client's own
//! sequence number. Sessions are SmartSDR v4 first (TCP API 1.4; maintainer ruling 2026-10-03):
//! the bundled [`Session::v4_gui_client`] is a 4.x radio with Nexus registering as a GUI client.
//!
//! # The faults, and the guard each one exists to trip
//!
//! Every fault is a positive control for a guard in the Flex client: the guard's test runs it
//! against the fault and must go red when the guard is removed. Details on [`Fault`].
//!
//! | Fault | What the client sees | The guard |
//! |---|---|---|
//! | [`Fault::Vita`] | VITA packets dropped or swapped in transit, gaps in the packet count | Receive continuity: a lost audio packet becomes silence and a late one is dropped; an FFT frame missing a fragment is never shown |
//! | [`Fault::SplitLine`] | a status or reply line delivered in pieces, the rest held until the client's next command | Line assembly never returns a partial line as a value |
//! | [`Fault::ReorderReply`] | one reply arrives after the next one | Replies matched by sequence number |
//! | [`Fault::DropPings`] | ping replies missing; the radio's keepalive closes a silent session | Keepalive: one missed reply survives, five end the session; a missed ping during an over unkeys |
//! | [`Fault::StuckTransmit`] | `xmit 0` answered with success, but the interlock stays TRANSMITTING, also after a reconnect | The unkey readback: keyed clears only on the interlock sequence; past the deadline, unkey again, drop the session, tell the operator |
//! | [`Fault::ForeignClient`] | another client's slice (the TX slice), pan and waterfall, optionally its transmission | Ownership by client handle: never retune, adopt or remove another client's objects; never key a transmitter that is not ours |
//! | [`Fault::DisconnectMidOver`] | the TCP session closes while the interlock reports TRANSMITTING | A lost session while keyed unkeys locally first, stops DAX TX and reconnects without swapping under a keyed transmitter |
//!
//! # The recorder
//!
//! [`record::record`] (the `flexrecord` binary) connects to a real radio as an ordinary, non-GUI
//! client and can send only the verbs in [`record::ALLOWED_VERBS`]: subscriptions, three read-only
//! queries, its UDP port and pings. Nothing transmit-capable or state-changing can be expressed
//! by its command type, its single write path checks the list again, and a test pins the list.
//! IP and MAC addresses, serial numbers and other identifiers are replaced ([`scrub`]) before
//! anything is written, and the file stays with the tester.
//!
//! # Provenance
//!
//! Written for Nexus from the protocol authority in the Flex client port plan
//! (`docs/specs/flex-client-port-plan.md` §4): FlexRadio's public API documentation (the
//! `smartsdr-api-docs` wiki at `c114d35`) and AetherSDR's tested 4.x behaviour as that plan cites
//! it (`aethersdr/AetherSDR` at `32fa50e4`), used as protocol facts. No code or fixture is ported
//! or copied from either. AetherSDR's demo radio speaks no wire protocol and is not used, and
//! flex-sim (GPL-3.0, Python) was read as a reference only. "FlexRadio" and "SmartSDR" are
//! trademarks of FlexRadio Systems, used here only to say what the simulator stands in for.

pub mod fault;
pub mod line;
pub mod record;
pub mod scrub;
pub mod server;
pub mod session;
pub mod vita;

pub use fault::{Fault, Foreign};
pub use server::{Closer, Config, Event, Logged, Simulator};
pub use session::Session;
pub use vita::{Content, Start, Stream};
