//! Ownership as pure policy functions: whose a pan, slice or stream is, what a create reply names,
//! which radio-side display objects have leaked, and how to recover a slice the radio did not
//! restore.
//!
//! Each function takes plain values and returns a decision; none of them does I/O or holds
//! state, and each has its own tests. The decisions are by `client_handle`, parsed strictly: an
//! owner that does not parse is never ours.
//!
//! - [`classify_owned_status`]: Defer, Claim, Apply, Ignore or Remove a status for an object.
//! - [`stream_status_belongs_to_us`], [`is_dead_orphan_dax_rx_status`],
//!   [`dax_tx_status_can_update_local_state`]: stream ownership, including the dead orphan
//!   (`client_handle=0` with `ip=0.0.0.0`).
//! - [`parse_create_response_stream_id`], [`parse_panafall_create_pan_id`]: ids from create
//!   replies.
//! - [`bounded_slice_capacity`]: the slice limit from the model table and the radio's report.
//! - [`display_inventory`]: the leaked-waterfall fingerprint (a waterfall whose pan is gone).
//! - [`slice_recreate`]: reuse a pan the radio restored instead of creating a duplicate.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), `src/models/RadioStatusOwnership.h`,
//! `src/core/StreamStatus.h`, `src/models/DisplayInventoryPolicy.h` and
//! `src/models/SliceRecreatePolicy.h` at commit `32fa50e4896a846a6970fa3f443bd49d667c139d`
//! (2026-10-03), translated from C++/Qt to Rust. Deliberate differences: handles and ids parse
//! strictly, so a malformed `client_handle` is never `0` and never ours (upstream's tolerant parse
//! read it as `0`, which `streamStatusBelongsToUs` then took as a legacy stream of ours), and an
//! owner of zero is never claimed, even while our own handle is unknown; ids are
//! numbers rather than strings; `interlockKeepsLocalTxOn` and `operatorTransmitActive` are not
//! ported, because transmit decisions stay in Nexus's engine; the remote-audio (Opus) tracking is
//! not ported, because that path serves SmartLink only. Recorded in the repo-root NOTICE
//! (AetherSDR entry).

use super::kv;
use super::wire::{parse_hex_u32, parse_id, Kvs};

/// `0x%08x`, the form an id is written back in.
pub fn hex_id(value: u32) -> String {
    format!("0x{value:08x}")
}

/// `%08x` for `stream remove`, or `None` for the never-valid id zero.
pub fn stream_command_id(value: u32) -> Option<String> {
    (value != 0).then(|| format!("{value:08x}"))
}

/// An id written with or without its `0x` prefix (a create reply may omit it): one to eight hex
/// digits.
fn parse_hex_id(text: &str) -> Option<u32> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    parse_hex_u32(digits, 8)
}

/// What to do with a status line for an object (a pan, a waterfall) that this client may own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedStatusAction {
    /// Not ours yet, and the line names no owner: wait for one that does.
    Defer,
    /// Not ours yet, and the line names us: take it.
    Claim,
    /// Already ours: apply the line.
    Apply,
    /// Another client's, or an owner that does not parse: leave it alone.
    Ignore,
    /// The object is gone.
    Remove,
}

/// Classify a status line for an object by its `client_handle`. Removal wins over ownership
/// uncertainty; a known object takes owner-less status; an owner that is not exactly our handle,
/// including one that does not parse, is ignored.
pub fn classify_owned_status(
    already_owned: bool,
    kvs: &Kvs,
    removed: bool,
    our_handle: u32,
) -> OwnedStatusAction {
    if removed {
        return OwnedStatusAction::Remove;
    }
    if !kvs.has("client_handle") {
        return if already_owned {
            OwnedStatusAction::Apply
        } else {
            OwnedStatusAction::Defer
        };
    }
    match kv::id(kvs, "client_handle") {
        Some(owner) if owner != 0 && owner == our_handle => {
            if already_owned {
                OwnedStatusAction::Apply
            } else {
                OwnedStatusAction::Claim
            }
        }
        _ => OwnedStatusAction::Ignore,
    }
}

/// Whether a stream's status may be taken as ours. A line with no `client_handle` is accepted for
/// compatibility with firmware that omits it; an owner of zero is accepted unless the endpoint is
/// the dead orphan's `0.0.0.0`; otherwise the owner must be our handle. An owner that does not
/// parse is not ours.
pub fn stream_status_belongs_to_us(kvs: &Kvs, our_handle: u32) -> bool {
    if !kvs.has("client_handle") {
        return true;
    }
    match kv::id(kvs, "client_handle") {
        Some(0) => kvs.get("ip").map(str::trim) != Some("0.0.0.0"),
        Some(owner) => owner == our_handle,
        None => false,
    }
}

/// A DAX RX stream the radio holds for no live client: owner zero and endpoint `0.0.0.0`.
pub fn is_dead_orphan_dax_rx_status(kvs: &Kvs) -> bool {
    kv::id(kvs, "client_handle") == Some(0) && kvs.get("ip").map(str::trim) == Some("0.0.0.0")
}

/// Whether a DAX TX stream's status may update local state: always for the stream already
/// created, otherwise only when it names our handle. A missing or zero owner never adopts an
/// unknown stream during startup.
pub fn dax_tx_status_can_update_local_state(
    stream_id: u32,
    current_stream_id: u32,
    kvs: &Kvs,
    our_handle: u32,
) -> bool {
    if current_stream_id != 0 && stream_id == current_stream_id {
        return true;
    }
    matches!(kv::id(kvs, "client_handle"), Some(owner) if owner != 0 && owner == our_handle)
}

/// The stream id in a `stream create` reply: `stream=<id>`, `id=<id>`, or the bare id, hex with
/// or without `0x`.
pub fn parse_create_response_stream_id(body: &str) -> Option<u32> {
    let kvs = Kvs::parse(body);
    let id = if kvs.has("stream") {
        kvs.get("stream").and_then(parse_hex_id)
    } else if kvs.has("id") {
        kvs.get("id").and_then(parse_hex_id)
    } else {
        parse_hex_id(body)
    };
    id.filter(|v| *v != 0)
}

/// The pan id in a `display panafall create` reply: `pan=<id>`, `id=<id>`, or the first of the
/// comma-separated `<pan>,<waterfall>` pair.
pub fn parse_panafall_create_pan_id(body: &str) -> Option<u32> {
    let kvs = Kvs::parse(body);
    if kvs.has("pan") {
        return kvs.get("pan").and_then(parse_hex_id);
    }
    if kvs.has("id") {
        return kvs.get("id").and_then(parse_hex_id);
    }
    body.trim().split(',').next().and_then(parse_hex_id)
}

/// One part of a `stream <id> [action]` object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamObjectParts {
    pub stream_id: u32,
    pub action: String,
}

/// Split `<prefix> <0xid> [action]`. `None` unless the id parses and is nonzero.
pub fn parse_stream_object(object: &str, prefix: &str) -> Option<StreamObjectParts> {
    let rest = object.strip_prefix(prefix)?.strip_prefix(' ')?.trim();
    let (id, action) = match rest.split_once(' ') {
        Some((id, action)) => (id, action.trim()),
        None => (rest, ""),
    };
    let stream_id = parse_id(id).filter(|v| *v != 0)?;
    Some(StreamObjectParts {
        stream_id,
        action: action.to_string(),
    })
}

/// A stream status that says the stream is gone: a `removed` action or key, or `in_use=0`.
pub fn status_removed(stream: &StreamObjectParts, kvs: &Kvs) -> bool {
    stream.action == "removed" || kvs.has("removed") || kvs.get("in_use") == Some("0")
}

/// The slice capacity: never above the model table's limit, raised toward it by what the radio
/// reports it can still create (`slices=N`), and unchanged until the model's limit is known.
pub fn bounded_slice_capacity(
    model_limit: i32,
    current_max: i32,
    current_slice_count: i32,
    reported_available: i32,
) -> i32 {
    if model_limit <= 0 {
        return current_max;
    }
    let clamped_current = current_max.min(model_limit);
    if reported_available < 0 {
        return clamped_current;
    }
    let reported_total = current_slice_count + reported_available;
    model_limit.min(clamped_current.max(reported_total))
}

/// One display object as the radio reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioPan {
    pub id: u32,
    pub client_handle: u32,
}

/// One waterfall as the radio reports it; `parent` is its `panadapter=` (`None` when unreported).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioWaterfall {
    pub id: u32,
    pub client_handle: u32,
    pub parent: Option<u32>,
}

/// Whose a display object is, for the inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryOwner {
    /// This client holds it.
    Ours,
    /// Another client's handle, provably (we know our own).
    Foreign,
    /// No live owner here: leaked, from a crashed session, or unclaimed.
    Orphan,
}

/// What the inventory is computed from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InventoryInputs {
    pub radio_pans: Vec<RadioPan>,
    pub radio_waterfalls: Vec<RadioWaterfall>,
    pub owned_pans: Vec<u32>,
    pub owned_waterfalls: Vec<u32>,
    /// `0` when unknown.
    pub our_handle: u32,
}

/// The inventory's verdicts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InventoryReport {
    pub pans: Vec<(u32, InventoryOwner)>,
    pub waterfalls: Vec<(u32, InventoryOwner)>,
    /// Waterfalls the radio still holds whose parent pan is gone: leaked display resources.
    pub leaked_waterfalls: Vec<u32>,
}

impl InventoryReport {
    pub fn count_pans(&self, owner: InventoryOwner) -> usize {
        self.pans.iter().filter(|(_, o)| *o == owner).count()
    }

    pub fn count_waterfalls(&self, owner: InventoryOwner) -> usize {
        self.waterfalls.iter().filter(|(_, o)| *o == owner).count()
    }
}

fn inventory_owner(id: u32, handle: u32, owned: &[u32], ours: u32) -> InventoryOwner {
    if owned.contains(&id) {
        InventoryOwner::Ours
    } else if handle != 0 && ours != 0 && handle != ours {
        // Foreign is asserted only when our own handle is known; otherwise a mismatch is
        // unprovable and the object is an orphan.
        InventoryOwner::Foreign
    } else {
        InventoryOwner::Orphan
    }
}

/// Classify every pan and waterfall the radio reports against what this client owns, and name
/// the waterfalls whose parent pan the radio no longer has. A waterfall with no reported parent
/// cannot be proven leaked and is not flagged.
pub fn display_inventory(inputs: &InventoryInputs) -> InventoryReport {
    let mut report = InventoryReport::default();
    for p in &inputs.radio_pans {
        let owner = inventory_owner(p.id, p.client_handle, &inputs.owned_pans, inputs.our_handle);
        report.pans.push((p.id, owner));
    }
    for w in &inputs.radio_waterfalls {
        let owner = inventory_owner(
            w.id,
            w.client_handle,
            &inputs.owned_waterfalls,
            inputs.our_handle,
        );
        report.waterfalls.push((w.id, owner));
        let parent_missing = w
            .parent
            .is_some_and(|parent| !inputs.radio_pans.iter().any(|p| p.id == parent));
        if parent_missing {
            report.leaked_waterfalls.push(w.id);
        }
    }
    report
}

/// How to recover a slice when `slice list` comes back empty at connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceRecreateAction {
    /// The radio restored one of our pans: put the slice on it rather than creating a duplicate.
    ReuseRestoredPan,
    /// No pan of ours exists: create one, then the slice on it.
    CreateNewPan,
}

/// What the decision is made from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliceRecreateInputs {
    pub has_restored_pan: bool,
    /// The restored pan's reported center, MHz; `<= 0` when not known.
    pub restored_pan_center_mhz: f64,
    /// The last frequency this client used, MHz; `<= 0` when unset.
    pub last_freq_mhz: f64,
    /// The last mode this client used; empty when unset.
    pub last_mode: String,
}

/// The decision.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceRecreateDecision {
    pub action: SliceRecreateAction,
    pub freq_mhz: f64,
    pub mode: String,
    pub antenna: String,
}

/// The fallback frequency when nothing better is known, MHz.
pub const DEFAULT_SLICE_FREQ_MHZ: f64 = 14.225;

/// Reuse a restored pan at its own center (radio-authoritative: the last frequency may be on
/// another band and outside the span); fall back to the last frequency, then the default. A new
/// pan starts at the last frequency or the default. Mode defaults to USB, antenna to ANT1.
pub fn slice_recreate(inputs: &SliceRecreateInputs) -> SliceRecreateDecision {
    let mode = if inputs.last_mode.is_empty() {
        "USB".to_string()
    } else {
        inputs.last_mode.clone()
    };
    let fallback = if inputs.last_freq_mhz > 0.0 {
        inputs.last_freq_mhz
    } else {
        DEFAULT_SLICE_FREQ_MHZ
    };
    let (action, freq_mhz) = if inputs.has_restored_pan {
        let freq = if inputs.restored_pan_center_mhz > 0.0 {
            inputs.restored_pan_center_mhz
        } else {
            fallback
        };
        (SliceRecreateAction::ReuseRestoredPan, freq)
    } else {
        (SliceRecreateAction::CreateNewPan, fallback)
    };
    SliceRecreateDecision {
        action,
        freq_mhz,
        mode,
        antenna: "ANT1".to_string(),
    }
}

#[cfg(test)]
mod tests {
    //! Translated from upstream's `radio_status_ownership_test.cpp` (the pan, stream, DAX TX,
    //! create-reply and capacity blocks; the interlock, operator-transmit, remote-audio and DAX TX
    //! platform blocks test code that is not ported), `display_inventory_policy_test.cpp` and
    //! `slice_recreate_policy_test.cpp`.
    use super::*;

    const OURS: u32 = 0x1234_5678;

    fn kvs(text: &str) -> Kvs {
        Kvs::parse(text)
    }

    #[test]
    fn panadapter_ownership_decisions() {
        // testPanadapterOwnershipDecisions.
        let none = kvs("");
        let ours = kvs("client_handle=0x12345678");
        let other = kvs("client_handle=0x87654321");
        use OwnedStatusAction::*;
        assert_eq!(classify_owned_status(false, &none, false, OURS), Defer);
        assert_eq!(classify_owned_status(false, &ours, false, OURS), Claim);
        assert_eq!(classify_owned_status(false, &other, false, OURS), Ignore);
        assert_eq!(classify_owned_status(true, &none, false, OURS), Apply);
        assert_eq!(classify_owned_status(true, &other, false, OURS), Ignore);
        assert_eq!(classify_owned_status(false, &none, true, OURS), Remove);
        // Strict: an owner that does not parse is never claimed, and neither is owner zero, even
        // when our own handle is unknown (zero).
        assert_eq!(
            classify_owned_status(false, &kvs("client_handle=garbage"), false, OURS),
            Ignore
        );
        assert_eq!(
            classify_owned_status(false, &kvs("client_handle=0x00000000"), false, 0),
            Ignore
        );
    }

    #[test]
    fn stream_status_ownership_compatibility() {
        // testStreamStatusOwnershipCompatibility.
        let none = kvs("");
        let unknown = kvs("client_handle=0x00000000");
        let orphan = kvs("client_handle=0x00000000 ip=0.0.0.0");
        let ours = kvs("client_handle=0x12345678");
        let other = kvs("client_handle=0x87654321");
        assert!(stream_status_belongs_to_us(&none, OURS));
        assert!(stream_status_belongs_to_us(&unknown, OURS));
        assert!(!stream_status_belongs_to_us(&orphan, OURS));
        assert!(stream_status_belongs_to_us(&ours, OURS));
        assert!(!stream_status_belongs_to_us(&other, OURS));
        assert!(is_dead_orphan_dax_rx_status(&orphan));
        assert!(!is_dead_orphan_dax_rx_status(&unknown));
        // The deliberate difference: upstream read a malformed owner as zero, and zero with a live
        // endpoint as a legacy stream of ours. A malformed owner is not ours.
        assert!(!stream_status_belongs_to_us(
            &kvs("client_handle=garbage ip=10.0.0.2"),
            OURS
        ));
        assert!(!stream_status_belongs_to_us(
            &kvs("client_handle=12345678"),
            OURS
        ));
    }

    #[test]
    fn dax_tx_status_ownership() {
        // testDaxTxStatusOwnership.
        let current = 0x0400_0001;
        let incoming = 0x0400_0002;
        assert!(!dax_tx_status_can_update_local_state(
            incoming,
            0,
            &kvs(""),
            OURS
        ));
        assert!(!dax_tx_status_can_update_local_state(
            incoming,
            0,
            &kvs("client_handle=0x00000000"),
            OURS
        ));
        assert!(dax_tx_status_can_update_local_state(
            incoming,
            0,
            &kvs("client_handle=0x12345678"),
            OURS
        ));
        assert!(!dax_tx_status_can_update_local_state(
            incoming,
            current,
            &kvs("client_handle=0x87654321"),
            OURS
        ));
        assert!(dax_tx_status_can_update_local_state(
            current,
            current,
            &kvs("client_handle=0x87654321"),
            OURS
        ));
        assert!(dax_tx_status_can_update_local_state(
            current,
            current,
            &kvs(""),
            OURS
        ));
    }

    #[test]
    fn create_reply_parsing() {
        // testCreateResponseParsing.
        assert_eq!(
            parse_create_response_stream_id("4000009"),
            Some(0x0400_0009)
        );
        assert_eq!(
            parse_create_response_stream_id("400000A"),
            Some(0x0400_000A)
        );
        assert_eq!(
            parse_create_response_stream_id("stream=0x0400000B"),
            Some(0x0400_000B)
        );
        assert_eq!(
            parse_create_response_stream_id("stream=4000009"),
            Some(0x0400_0009)
        );
        assert_eq!(stream_command_id(0x0400_000A).as_deref(), Some("0400000a"));
        assert_eq!(stream_command_id(0), None);
        assert_eq!(
            parse_panafall_create_pan_id("0x40000000,0x42000000"),
            Some(0x4000_0000)
        );
        assert_eq!(
            parse_panafall_create_pan_id("40000000,0x42000000"),
            Some(0x4000_0000)
        );
        assert_eq!(
            parse_panafall_create_pan_id("pan=0x40000001 waterfall=0x42000001"),
            Some(0x4000_0001)
        );
        // Strict: garbage is no id, and zero is never one.
        assert_eq!(parse_create_response_stream_id("garbage"), None);
        assert_eq!(parse_create_response_stream_id("0x0"), None);
        assert_eq!(parse_panafall_create_pan_id(""), None);
        assert_eq!(hex_id(0x4000_0000), "0x40000000");
    }

    #[test]
    fn stream_objects() {
        let p = parse_stream_object("stream 0x0400000B removed", "stream").unwrap();
        assert_eq!((p.stream_id, p.action.as_str()), (0x0400_000B, "removed"));
        assert!(status_removed(&p, &kvs("")));
        let p = parse_stream_object("stream 0x0400000B", "stream").unwrap();
        assert!(!status_removed(&p, &kvs("type=dax_rx")));
        assert!(status_removed(&p, &kvs("in_use=0")));
        assert_eq!(parse_stream_object("stream garbage", "stream"), None);
        assert_eq!(parse_stream_object("streamer 0x1", "stream"), None);
    }

    #[test]
    fn bounded_slice_capacity_rows() {
        // testBoundedSliceCapacity.
        assert_eq!(bounded_slice_capacity(8, 8, 8, 2), 8);
        assert_eq!(bounded_slice_capacity(8, 10, 8, 2), 8);
        assert_eq!(bounded_slice_capacity(8, 10, 1, 2), 8);
        assert_eq!(bounded_slice_capacity(8, 10, 1, -1), 8);
        assert_eq!(bounded_slice_capacity(4, 10, 1, 2), 4);
        assert_eq!(bounded_slice_capacity(8, 4, 3, 5), 8);
        assert_eq!(bounded_slice_capacity(4, 4, 1, -1), 4);
        assert_eq!(bounded_slice_capacity(0, 4, 8, 2), 4);
    }

    fn pan(id: u32, h: u32) -> RadioPan {
        RadioPan {
            id,
            client_handle: h,
        }
    }

    fn wf(id: u32, h: u32, parent: Option<u32>) -> RadioWaterfall {
        RadioWaterfall {
            id,
            client_handle: h,
            parent,
        }
    }

    #[test]
    fn inventory_clean_single_pan() {
        // testCleanSinglePan.
        let r = display_inventory(&InventoryInputs {
            radio_pans: vec![pan(0x4000_0000, 0x1111_1111)],
            radio_waterfalls: vec![wf(0x4200_0000, 0x1111_1111, Some(0x4000_0000))],
            owned_pans: vec![0x4000_0000],
            owned_waterfalls: vec![0x4200_0000],
            our_handle: 0x1111_1111,
        });
        assert_eq!(r.count_pans(InventoryOwner::Ours), 1);
        assert_eq!(r.count_waterfalls(InventoryOwner::Ours), 1);
        assert_eq!(r.count_pans(InventoryOwner::Orphan), 0);
        assert!(r.leaked_waterfalls.is_empty());
    }

    #[test]
    fn inventory_leaked_waterfall_with_its_parent_gone() {
        // testLeakedWaterfallParentGone: the pan was removed, its waterfall lingers.
        let r = display_inventory(&InventoryInputs {
            radio_pans: vec![pan(0x4000_0000, 0x1111_1111)],
            radio_waterfalls: vec![
                wf(0x4200_0000, 0x1111_1111, Some(0x4000_0000)),
                wf(0x4200_0001, 0x1111_1111, Some(0x4000_0001)),
            ],
            owned_pans: vec![0x4000_0000],
            owned_waterfalls: vec![0x4200_0000],
            our_handle: 0x1111_1111,
        });
        assert_eq!(r.leaked_waterfalls, [0x4200_0001]);
        assert_eq!(r.count_waterfalls(InventoryOwner::Orphan), 1);
    }

    #[test]
    fn inventory_foreign_is_not_orphan() {
        // testForeignNotOrphan.
        let r = display_inventory(&InventoryInputs {
            radio_pans: vec![pan(0x4000_0000, 0x1111_1111), pan(0x4000_0005, 0x2222_2222)],
            radio_waterfalls: vec![
                wf(0x4200_0000, 0x1111_1111, Some(0x4000_0000)),
                wf(0x4200_0005, 0x2222_2222, Some(0x4000_0005)),
            ],
            owned_pans: vec![0x4000_0000],
            owned_waterfalls: vec![0x4200_0000],
            our_handle: 0x1111_1111,
        });
        assert_eq!(r.count_pans(InventoryOwner::Foreign), 1);
        assert_eq!(r.count_waterfalls(InventoryOwner::Foreign), 1);
        assert_eq!(r.count_pans(InventoryOwner::Orphan), 0);
        assert!(r.leaked_waterfalls.is_empty());
    }

    #[test]
    fn inventory_unreported_parent_and_unknown_handle() {
        // testNoParentReportedNotLeaked.
        let r = display_inventory(&InventoryInputs {
            radio_pans: vec![pan(0x4000_0000, 0x1111_1111)],
            radio_waterfalls: vec![wf(0x4200_0000, 0x1111_1111, None)],
            owned_pans: vec![0x4000_0000],
            owned_waterfalls: vec![0x4200_0000],
            our_handle: 0x1111_1111,
        });
        assert!(r.leaked_waterfalls.is_empty());
        // testUnknownOurHandleFallsToOrphan.
        let r = display_inventory(&InventoryInputs {
            radio_pans: vec![pan(0x4000_0005, 0x2222_2222)],
            ..InventoryInputs::default()
        });
        assert_eq!(r.count_pans(InventoryOwner::Orphan), 1);
        assert_eq!(r.count_pans(InventoryOwner::Foreign), 0);
    }

    fn recreate(restored: bool, center: f64, last: f64, mode: &str) -> SliceRecreateDecision {
        slice_recreate(&SliceRecreateInputs {
            has_restored_pan: restored,
            restored_pan_center_mhz: center,
            last_freq_mhz: last,
            last_mode: mode.to_string(),
        })
    }

    #[test]
    fn slice_recreate_rows() {
        // testIssue3212BundleScenario: reuse the restored pan, at its center, not at a stale
        // frequency on another band.
        let d = recreate(true, 14.282408, 28.305, "USB");
        assert_eq!(d.action, SliceRecreateAction::ReuseRestoredPan);
        assert_eq!(d.freq_mhz, 14.282408);
        assert_eq!((d.mode.as_str(), d.antenna.as_str()), ("USB", "ANT1"));
        // testNoRestoredPanCreatesNew.
        let d = recreate(false, 0.0, 7.15, "LSB");
        assert_eq!(d.action, SliceRecreateAction::CreateNewPan);
        assert_eq!((d.freq_mhz, d.mode.as_str()), (7.15, "LSB"));
        // testNoRestoredPanNoSettingsUsesDefaults.
        let d = recreate(false, 0.0, 0.0, "");
        assert_eq!(d.action, SliceRecreateAction::CreateNewPan);
        assert_eq!((d.freq_mhz, d.mode.as_str()), (14.225, "USB"));
        // testRestoredPanCenterZeroFallsBack.
        let d = recreate(true, 0.0, 21.3, "USB");
        assert_eq!(
            (d.action, d.freq_mhz),
            (SliceRecreateAction::ReuseRestoredPan, 21.3)
        );
        let d = recreate(true, 0.0, 0.0, "");
        assert_eq!(
            (d.action, d.freq_mhz),
            (SliceRecreateAction::ReuseRestoredPan, 14.225)
        );
        // testRestoredPanCenterWinsOverLastFrequency.
        assert_eq!(recreate(true, 14.25, 14.074, "USB").freq_mhz, 14.25);
    }
}
