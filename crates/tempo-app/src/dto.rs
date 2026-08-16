//! Serializable data-transfer objects that form the wire contract between the
//! Rust application logic and the frontend.
//!
//! Every type here serializes to JSON with **camelCase** field names so the
//! TypeScript mock and the real engine share one shape. These DTOs are pure
//! data: they carry no behavior and depend only on `serde`. [`crate::AppState`]
//! projects the richer `tempo-core` types into these for the UI.

use modes::ModeKind;
use serde::{Deserialize, Serialize};

/// How recently a station was last heard, bucketed for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Presence {
    Active,
    Idle,
    Stale,
}

/// Geography-based rarity of a Maidenhead grid square. Mirrors
/// `propagation::gridrarity::GridRarity` (identical serde strings) — tempo-app
/// has no propagation dependency, so the tier arrives through the injected
/// resolver closure as 0–3 and is mapped here (like the DXCC resolver pattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridRarity {
    Common,
    Uncommon,
    Rare,
    UltraRare,
}

impl GridRarity {
    /// Map the injected resolver's raw 0–3 tier.
    pub fn from_tier(t: u8) -> Self {
        match t {
            3 => GridRarity::UltraRare,
            2 => GridRarity::Rare,
            1 => GridRarity::Uncommon,
            _ => GridRarity::Common,
        }
    }
}

/// A station in the roster / presence list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub call: String,
    pub grid: Option<String>,
    pub snr: i32,
    pub last_heard_slot: u64,
    pub heard_count: u32,
    pub presence: Presence,
    /// True if this callsign is in the logbook (worked before) — for B4 styling.
    pub worked: bool,
    /// DXCC entity name (country), resolved from the callsign — DX chasers scan
    /// the roster by country. `None` unless a DXCC resolver is wired.
    #[serde(default)]
    pub country: Option<String>,
    /// The tier/protocol this station was last heard on (FT1 = Tempo, FT8/FT4 = digital
    /// ops). `None` for DX1/unknown. The Tempo roster shows only FT1 stations; Operate
    /// shows all.
    #[serde(default)]
    pub tier: Option<Tier>,
    /// Geography-based rarity of the station's grid. `None` when grid-less or
    /// no rarity resolver is wired (headless tests).
    #[serde(default)]
    pub grid_rarity: Option<GridRarity>,
    /// The station uploads to LoTW (within the operator's recency window) —
    /// false when the user-activity file hasn't been fetched (honest default).
    #[serde(default)]
    pub lotw_user: bool,
    /// Audio offset (Hz) of the station's last decode — where on the waterfall they were
    /// heard. A roster click passes it so RX/TX move there, exactly like a Band Activity
    /// double-click (`call_station_ctx` then applies the Hold-Tx rule). `None` when the
    /// station is known only from free-text attribution.
    #[serde(default)]
    pub freq_hz: Option<i32>,
    /// Who this station is calling — the addressee of its last structured frame. `None`
    /// means it addressed nobody (a CQ), which the roster renders as "CQ". Lets the
    /// operator see who is already engaged before double-clicking a row.
    #[serde(default)]
    pub calling: Option<String>,
    /// Primary administrative subdivision — a US state or a Canadian province, as the ADIF
    /// `STATE` code either way. From the callsign (the FCC index / the Canadian regional
    /// numeral) or the heard grid: the SAME hint the needed board and WAS use, never a
    /// callbook lookup. `None` for a station in neither country, or when no resolver is wired.
    /// Stamped by the engine snapshot loop (the resolver lives there).
    ///
    /// Named `state` and not `us_state` because it stopped being US-only, and because ADIF's
    /// own `STATE` means exactly this: a field whose name promises a country it no longer
    /// keeps is how two readers end up guessing different things off one wire.
    #[serde(default)]
    pub state: Option<String>,
}

/// A single decoded signal from the most recent RX slot, for the live decode
/// feed (alerts + color-coding). Distinct from `ChatMessage` (which is threaded
/// conversation): this is the raw heard-this-slot list, like WSJT-X Band Activity.
/// The pending hunt target shown as a chip ("hunting K-1234 · W1ABC").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuntDto {
    pub program: String,
    pub reference: String,
    pub call: String,
}

/// One UDP-driven callsign highlight (JTAlert paints wanted/B4 calls).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightEntry {
    pub call: String,
    pub bg: Option<String>,
    pub fg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeRow {
    /// Sender callsign if parsed from the message, else None.
    pub from: Option<String>,
    pub snr: i32,
    pub dt_sec: f32,
    pub freq_hz: f32,
    pub message: String,
    /// True if this is a CQ call.
    pub is_cq: bool,
    /// True if addressed to my callsign (someone calling me).
    pub directed_to_me: bool,
    /// True if this is a QSO-ending signoff (`RR73` / `73`) — classified by the
    /// parse (`Msg::is_signoff`), token-positional, so a `DM73` grid never counts.
    /// Drives the Band Activity CQ+73 chip: a signoff means a frequency is about
    /// to free up. `RRR` stays off (its QSO still has a 73 coming).
    #[serde(default)]
    pub signoff: bool,
    /// True if the sender is in the logbook (worked before).
    pub worked: bool,
    /// Sender's DXCC entity name (country), resolved from the callsign. `None`
    /// unless a DXCC resolver is wired (always None in headless tests). DX chasers
    /// scan by country, so this rides on every decode + roster row.
    #[serde(default)]
    pub country: Option<String>,
    /// True if the sender resolves to a DXCC entity never worked ON ANY BAND — a true all-time
    /// "new one" (ATNO), matching the Needed board's NEW ONE. Off unless a DXCC resolver is wired
    /// (always off in headless tests).
    #[serde(default)]
    pub new_dxcc: bool,
    /// True if the sender's entity IS worked (somewhere) but NOT on the current band — a new
    /// band-slot (DXCC is awarded per band). Mutually exclusive with `new_dxcc`.
    #[serde(default)]
    pub new_band: bool,
    /// True if the sender's entity is CONFIRMED (award-grade) on this band — for the
    /// decode panes' hide-confirmed filter (F4MQS). Never true when new_dxcc/new_band is.
    #[serde(default)]
    pub confirmed_band: bool,
    /// True if the decode carries a Maidenhead grid never worked before.
    #[serde(default)]
    pub new_grid: bool,
    /// The grid the decode carried (CQ/grid messages), for alert copy + rarity.
    #[serde(default)]
    pub grid: Option<String>,
    /// Geography-based rarity of that grid — lets the rare ones alert loudly
    /// while plain new-grids stay quiet. `None` when grid-less or unwired.
    #[serde(default)]
    pub grid_rarity: Option<GridRarity>,
    /// The sender uploads to LoTW (within the operator's recency window) —
    /// the award-chaser's "this contact will confirm" mark. False when the
    /// user-activity file hasn't been fetched (honest default: no highlight).
    #[serde(default)]
    pub lotw_user: bool,
    /// True if this row is OUR OWN transmitted message (not a received decode) —
    /// the UI shows it highlighted (yellow) and one row per cycle, so the operator
    /// sees each of their calls. `snr`/`dt_sec` are 0 and `rv` is -1 for these.
    #[serde(default)]
    pub mine: bool,
    /// For `mine` rows: the Unix-second the message was transmitted. STABLE per
    /// transmission, so the UI keys/timestamps each own-TX row by its real cycle
    /// (not the browser clock) — one row per actual transmission, no dupes. `None`
    /// for received decodes.
    #[serde(default)]
    pub tx_at: Option<u64>,
    pub tier: Tier,
    /// WSJT-X 'a' marker: the decode used a-priori (AP) assistance.
    #[serde(default)]
    pub ap: bool,
    /// WSJT-X '?' marker: low-confidence decode (quality below the stock line).
    #[serde(default)]
    pub low_conf: bool,
    /// IR-HARQ redundancy versions combined to recover this decode: 0 = decoded
    /// from the initial transmission alone; 1/2 = recovered by joint-combining
    /// that many retransmissions; -1 = not applicable (e.g. DX1). Lets the UI
    /// badge HARQ-recovered decodes.
    pub rv: i32,
}

/// The radio-frequency / signal tier a message or link is using.
///
/// `Ft1` is the fast 4 s coherent tier; `Dx1` is the non-coherent, fading-
/// resilient 15 s robust tier. `Ft8` (15 s) and `Ft4` (7.5 s) are the standard
/// WSJT-X modes — now live-selectable via the native `modes` decode/encode
/// pipeline. [`Tier::mode_kind`] maps each native tier to its [`ModeKind`]; `Dx1`
/// maps to `None` (it uses FT1's robust non-coherent path, not a `modes::Mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Tier {
    #[default]
    #[serde(rename = "TempoFast")]
    TempoFast,
    #[serde(rename = "TempoDeep")]
    TempoDeep,
    #[serde(rename = "FT8")]
    Ft8,
    #[serde(rename = "FT4")]
    Ft4,
    /// WSJT-X FST4 (QSO mode) — **transmits and receives**, verified on-air
    /// compatible by having stock WSJT-X `jt9 -7` decode our transmission at every
    /// period. The T/R period comes from `Settings::fst4_period_s`.
    #[serde(rename = "FST4")]
    Fst4,
    /// WSJT-X FST4W — the WSPR-like BEACON mode (50-bit messages, no AP
    /// decoding). **RECEIVE-ONLY**, unlike FST4, which transmits. The C ABI can
    /// encode it (`iwspr=1`); what is missing is the operating layer — a beacon
    /// needs a transmit-percentage scheduler and a callsign/grid/power message,
    /// not the QSO sequencer.
    ///
    /// A separate tier rather than a flag on `Fst4` because it is a separate
    /// operator decision: FST4 is for working a station, FST4W is for listening
    /// to beacons and building propagation evidence. They share one decoder, one
    /// frame contract, and `Settings::fst4_period_s`.
    #[serde(rename = "FST4W")]
    Fst4w,
    /// WSJT-X Q65 — EME and VHF+ scatter. **Transmits and receives**: the encoder
    /// (`q65_encode_msg` / `q65_gen_wave`) is verified on-air-compatible by having
    /// stock WSJT-X `jt9` decode our transmission at every period and submode.
    ///
    /// Plain "Q65", NOT a period/submode-qualified name. The tier is the
    /// operator's mode selection; which of the 25 period/submode combinations it
    /// resolves to comes from `Settings::q65_period_s` / `q65_submode`. Baking a
    /// combination into the tier name would make the label lie the moment the
    /// operator changed the period — the qualified name lives on `ModeKind`,
    /// which actually knows.
    #[serde(rename = "Q65")]
    Q65,
    /// WSJT-X MSK144 — the METEOR-SCATTER mode. **Transmits and receives**, verified
    /// against stock `jt9 -k` at every period. Keys for nearly the whole period,
    /// repeating one 72 ms frame — that repetition IS the mode. The T/R period comes
    /// from `Settings::msk144_period_s`.
    #[serde(rename = "MSK144")]
    Msk144,
    /// WSJT JT65 — the classic weak-signal / EME mode. **Receive-only in this build**:
    /// the encoder is verified but transmit is disabled pending a Windows crash on
    /// Call CQ. The submode comes from `Settings::jt65_submode`.
    #[serde(rename = "JT65")]
    Jt65,
    /// WSPR — the propagation-BEACON mode. **RECEIVE-ONLY**. Unlike every other
    /// tier, its decodes are propagation reports rather than QSO traffic.
    #[serde(rename = "WSPR")]
    Wspr,
}

impl Tier {
    /// Every variant, in declaration order. The one place a tier list lives, so
    /// a test can drive them all — see `bandplan::tests::tier_all_lists_every_tier`,
    /// which fails to compile if a variant is added without being listed here.
    pub const ALL: [Tier; 10] = [
        Tier::TempoFast,
        Tier::TempoDeep,
        Tier::Ft8,
        Tier::Ft4,
        Tier::Fst4,
        Tier::Fst4w,
        Tier::Q65,
        Tier::Msk144,
        Tier::Jt65,
        Tier::Wspr,
    ];
}

impl Tier {
    /// The operator-facing name — what the top-bar pill reads. Distinct from the
    /// ADIF mode name in `Engine::qso_record`, which must stay the registered one
    /// for award credit even where the two happen to agree.
    pub fn label(self) -> &'static str {
        match self {
            Tier::TempoFast => "Tempo Fast",
            Tier::TempoDeep => "Tempo Deep",
            Tier::Ft8 => "FT8",
            Tier::Ft4 => "FT4",
            Tier::Fst4 => "FST4",
            Tier::Fst4w => "FST4W",
            Tier::Q65 => "Q65",
            Tier::Msk144 => "MSK144",
            Tier::Jt65 => "JT65",
            Tier::Wspr => "WSPR",
        }
    }

    /// True for the Tempo CHAT-capable tiers (TempoFast/TempoDeep). The chat cadence —
    /// store-and-forward resends, delivery ACKs, conversation folding, and every behavior the
    /// 2026-07 cadence rework adds — runs ONLY on these tiers. Mode::Chat at an FT tier is a
    /// legitimate RESTING state (it's the boot state, pinned silent by
    /// `chat_mode_at_ft8_tier_stays_silent`), and it must stay inert: gate on this, never on
    /// `mode == Chat` alone.
    pub fn is_chat(self) -> bool {
        matches!(self, Tier::TempoFast | Tier::TempoDeep)
    }

    /// The native decode/encode mode this tier maps to, or `None` for `Dx1`
    /// (FT1's robust non-coherent tier, handled outside the `modes::Mode` set).
    ///
    /// `q65_period_s` / `q65_submode` come from settings and are used ONLY by the
    /// Q65 tier. Q65 is the first tier whose mode is not fully determined by the
    /// tier alone: the period sets the frame length and the slot clock, so it has
    /// to reach `ModeKind` or every buffer sized from that kind would be wrong.
    /// An out-of-range pair falls back to Q65-30A rather than refusing — settings
    /// arriving from an older file or a hand-edited JSON should degrade to a
    /// working mode, not disable decoding.
    pub fn mode_kind(
        self,
        q65_period_s: u16,
        q65_submode: u8,
        fst4_period_s: u16,
        msk144_period_s: u16,
        jt65_submode: u8,
    ) -> Option<ModeKind> {
        match self {
            Tier::TempoFast => Some(ModeKind::TempoFast),
            Tier::Ft8 => Some(ModeKind::Ft8),
            Tier::Ft4 => Some(ModeKind::Ft4),
            Tier::Fst4 => Some(Self::fst4_kind(fst4_period_s, false)),
            Tier::Fst4w => Some(Self::fst4_kind(fst4_period_s, true)),
            Tier::Q65 => Some(Self::q65_kind(q65_period_s, q65_submode)),
            Tier::Msk144 => Some(Self::msk144_kind(msk144_period_s)),
            Tier::Jt65 => Some(Self::jt65_kind(jt65_submode)),
            Tier::Wspr => Some(ModeKind::Wspr),
            Tier::TempoDeep => None,
        }
    }

    /// A validated `ModeKind::Jt65`, falling back to A on an unsupported submode.
    pub fn jt65_kind(submode: u8) -> ModeKind {
        if submode < ModeKind::JT65_SUBMODES {
            ModeKind::Jt65 { submode }
        } else {
            ModeKind::JT65A
        }
    }

    /// A validated `ModeKind::Msk144`, falling back to 15 s (the 6 m workhorse)
    /// on an unsupported period. Same degrade-don't-refuse rule as the others.
    pub fn msk144_kind(period_s: u16) -> ModeKind {
        if ModeKind::MSK144_PERIODS.contains(&period_s) {
            ModeKind::Msk144 { period_s }
        } else {
            ModeKind::MSK144_15
        }
    }

    /// A validated `ModeKind::Fst4`, falling back to 15 s on an unsupported period.
    /// Same degrade-don't-refuse rule as [`Self::q65_kind`].
    pub fn fst4_kind(period_s: u16, wspr: bool) -> ModeKind {
        if ModeKind::FST4_PERIODS.contains(&period_s) {
            ModeKind::Fst4 { period_s, wspr }
        } else {
            ModeKind::Fst4 { period_s: 15, wspr }
        }
    }

    /// A validated `ModeKind::Q65`, falling back to Q65-30A on anything unsupported.
    pub fn q65_kind(period_s: u16, submode: u8) -> ModeKind {
        let ok = ModeKind::Q65_PERIODS.contains(&period_s) && submode < ModeKind::Q65_SUBMODES;
        if ok {
            ModeKind::Q65 { period_s, submode }
        } else {
            ModeKind::Q65_30A
        }
    }

    /// The tier a decode's [`ModeKind`] belongs to (inverse of [`mode_kind`]).
    /// Lets the decode feed label each row by the mode that produced it.
    ///
    /// [`mode_kind`]: Tier::mode_kind
    pub fn from_mode_kind(kind: ModeKind) -> Tier {
        match kind {
            ModeKind::TempoFast => Tier::TempoFast,
            ModeKind::Ft8 => Tier::Ft8,
            ModeKind::Ft4 => Tier::Ft4,
            // FST4 and FST4W are distinct tiers; the wspr flag is what tells them
            // apart, and a decode row must be labelled with the one that produced it.
            ModeKind::Fst4 { wspr: false, .. } => Tier::Fst4,
            ModeKind::Fst4 { wspr: true, .. } => Tier::Fst4w,
            // Every Q65 combination maps back to the one Q65 tier: the tier is the
            // operator's mode selection, and period/submode are settings under it.
            ModeKind::Q65 { .. } => Tier::Q65,
            ModeKind::Msk144 { .. } => Tier::Msk144,
            ModeKind::Jt65 { .. } => Tier::Jt65,
            ModeKind::Wspr => Tier::Wspr,
        }
    }
}

/// A single chat message (inbound or outbound) within a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub from: Option<String>,
    pub to: Option<String>,
    pub text: String,
    pub slot: u64,
    pub directed_to_me: bool,
    pub outbound: bool,
    pub snr: Option<i32>,
    pub freq_hz: Option<f32>,
    pub dt_sec: Option<f32>,
    pub tier: Option<Tier>,
    /// `Some((have, tot))` for an INBOUND message that never fully arrived — the chunks that
    /// did land. The thread shows "2 of 3 received" rather than the message never appearing at
    /// all. A chat client does not silently swallow a message.
    #[serde(default)]
    pub incomplete: Option<(usize, usize)>,
    /// For an OUTBOUND directed message: the recipient acknowledged receipt (an id-bearing
    /// RR73 ACK came back). Drives a REAL "Delivered ✓" instead of the old heuristic.
    #[serde(default)]
    pub delivered: bool,
    /// For an OUTBOUND directed message: the store chunk-id char assigned to it, so an
    /// id-bearing ACK confirms exactly this message (no FIFO guessing). `None` for inbound
    /// + broadcasts.
    #[serde(default)]
    pub ack_id: Option<char>,
    /// For an OUTBOUND directed message: transmit cycles used so far (bounded by
    /// `chat_max_cycles`) — drives the bubble's "sending k/N". 0 while still held.
    #[serde(default)]
    pub attempts: u32,
    /// For an OUTBOUND directed message: implicitly confirmed — after it transmitted, the
    /// peer sent a COMPLETE directed message back to us (they demonstrably hear us). The
    /// resend schedule stopped. Weaker than `delivered` (shown as "confirmed", never
    /// "Delivered ✓" — that stays reserved for the id-bearing RR73).
    #[serde(default)]
    pub confirmed: bool,
    /// For an OUTBOUND directed message: terminal "sent N times, never acknowledged".
    /// The resend schedule stopped; a LATE RR73 within the grace window may still flip
    /// this to `delivered`. Drives the bubble's "no ack" state + tap-to-resend.
    #[serde(default)]
    pub no_ack: bool,
    /// For an OUTBOUND directed message: still HELD in the store-and-forward queue, never
    /// yet released on the air because the recipient hasn't been heard. Cleared the moment
    /// the message first transmits. `false` for inbound + broadcasts.
    ///
    /// This is the distinction the operator can't otherwise see: EVERY directed message goes
    /// through store-and-forward (`send_message` has no send-now path), so "held, going
    /// nowhere" and "transmitted, awaiting ACK" both used to render an identical "✓ Sent".
    #[serde(default)]
    pub stored: bool,
    /// For an OUTBOUND directed message: it was HELD when the app last closed and could NOT be
    /// restored to the store-and-forward queue — so it will never transmit, and the operator
    /// must re-send it. Set at load time. With the pending_msgs.json journal now persisting the
    /// queue, this is RARE: it fires only for a message queued before the journal existed, or a
    /// missing/corrupt journal. A held message that IS restored keeps `stored` and transmits
    /// when the peer is next heard (the point of store-and-forward).
    ///
    /// This exists so the app stops asserting something false. Clearing `stored` alone made a
    /// never-transmitted message render as a plain "Sent" — trading a VISIBLE broken promise
    /// for an INVISIBLE one, which is worse: the operator believes it went out and never re-sends.
    #[serde(default)]
    pub abandoned: bool,
}

/// A per-peer thread of chat messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub peer: String,
    pub messages: Vec<ChatMessage>,
}

/// Current state of the active link to a peer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkState {
    pub tier: Tier,
    /// The active tier's T/R period in seconds. The Fast Graph needs it (its X axis is one
    /// period wide) and the UI must not hardcode it — MSK144's is a 5/10/15/30 setting, and
    /// `decodeHistory.ts` mislabelled period boundaries for years by assuming 15.
    #[serde(default)]
    pub period_secs: f64,
    pub snr_db: f32,
    pub dt_sec: f32,
    pub freq_hz: f32,
    pub rv: i32,
    pub state: String,
    pub quality: f32,
}

/// Where the engine's decodes come from — the user-selectable native-vs-companion
/// switch. `Native` decodes locally captured audio; `Companion` rides an upstream
/// WSJT-X/JTDX/MSHV decode stream over UDP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    #[default]
    Native,
    Companion,
}

/// A compact per-radio summary for the multi-radio switcher (dual-radio). One per configured radio;
/// the ACTIVE radio carries live state, the others their last-known band/frequency (they're not
/// connected in the active-only model). Absent/1-element ⇒ the UI renders single-radio, unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioSummary {
    pub id: u32,
    pub name: String,
    pub band: String,
    pub dial_mhz: f64,
    pub sideband: String,
    pub is_active: bool,
    /// Live CAT health for the active radio; `None` for a not-connected radio.
    pub cat_ok: Option<bool>,
    /// Live S-meter (dB rel S9) for the active radio; `None` otherwise.
    pub smeter_db: Option<i32>,
    pub transmitting: bool,
    /// The bands this radio covers (empty = all) — for auto-routing (P4) + a coverage hint.
    pub bands: Vec<String>,
}

/// Current radio / slot-timing status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStatus {
    pub dial_mhz: f64,
    pub band: String,
    pub sideband: String,
    /// The active operating SECTION ("digital" | "phone" | "cw" | "rtty") — live engine
    /// state, mirrored so surfaces reachable from any section (the Satellites log strip)
    /// can answer section-dependent questions (a digital-tier contact logs the TIER's
    /// mode, not the sideband it was generated on) without a settings round-trip.
    #[serde(default)]
    pub operating_mode: String,
    /// The rig keyed by something that is NOT Nexus (mic PTT / straight key), read back
    /// over CAT (#57). Display-only — feeds the TX badge and the meter pane.
    #[serde(default)]
    pub rig_keyed: bool,
    /// HRD Logbook link (F4MQS): `Some(true)` last datagram delivered, `Some(false)` HRD
    /// unreachable (contacts queued), `None` forwarding off or nothing sent yet.
    #[serde(default)]
    pub hrd_link_up: Option<bool>,
    /// QSOs queued for HRD because it was unreachable — 0 when caught up.
    #[serde(default)]
    pub hrd_queued: u32,
    pub transmitting: bool,
    pub slot: u64,
    pub next_slot_ms: u64,
    pub time_sync_ok: bool,
    /// RF output power fraction (0.0–1.0): the rig's read-back when CAT reports
    /// it, else the last commanded value; `None` until either exists.
    #[serde(default)]
    pub rf_power: Option<f32>,
    /// Mic gain as a 0.0–1.0 fraction — the rig's read-back, or the commanded value until the
    /// poll confirms it. `None` when the rig doesn't report it. Pairs with the ALC meter for
    /// SSB setup (raise mic gain until ALC peaks tickle the zone).
    #[serde(default)]
    pub mic_gain: Option<f32>,
    /// Noise-reduction level (0.0–1.0) — rig read-back or commanded; `None` when unsupported.
    #[serde(default)]
    pub nr_level: Option<f32>,
    /// AGC time constant as "fast"|"mid"|"slow"; `None` when the rig doesn't report it.
    #[serde(default)]
    pub agc: Option<String>,
    /// CAT S-meter reading in dB relative to S9 (S9 = 0 dB, S1 ≈ -48, S9+20 = +20).
    /// `None` when the rig doesn't report STRENGTH over CAT, so the UI shows no meter
    /// rather than a fake one. RX-only — not refreshed while transmitting.
    #[serde(default)]
    pub smeter_db: Option<i32>,
    /// Transmit meters from the rig's CAT poll (native Icom CI-V), refreshed ONLY while
    /// keyed and `None` while receiving — the mirror image of `smeter_db`. `tx_swr` is the
    /// SWR ratio (1.0–6.0), `tx_alc` is ALC 0.0–1.0, `tx_po_w` is output power in watts,
    /// `tx_comp_db` is speech compression in dB. Each is independently absent when the rig
    /// doesn't report it, so the UI shows only the meters it actually has.
    #[serde(default)]
    pub tx_swr: Option<f32>,
    #[serde(default)]
    pub tx_alc: Option<f32>,
    #[serde(default)]
    pub tx_po_w: Option<f32>,
    #[serde(default)]
    pub tx_comp_db: Option<f32>,
    /// The rig's actual mode read back over CAT (Hamlib name, e.g. "USB"/"LSB"/"FM").
    /// Display-only — the cockpit flags a mismatch with the commanded mode. `None` until
    /// the rig reports it.
    #[serde(default)]
    pub rig_mode: Option<String>,
    /// The operator's TRANSIENT Phone mode override ("USB"/"LSB"/"FM"), or `None` = AUTO
    /// (band-derived). Drives the cockpit mode picker's active highlight; cleared on a band change.
    #[serde(default)]
    pub sideband_override: Option<String>,
    /// The operator's PHONE (SSB/image) sub-band on the CURRENT band as an [lo, hi) MHz range,
    /// per their license class — the phone band-strip shades it so they see "where I may talk".
    /// `None` for a class with no phone privilege on the band, an Open (non-US) license, or an
    /// off-plan band. Both are set together.
    #[serde(default)]
    pub phone_seg_lo: Option<f64>,
    #[serde(default)]
    pub phone_seg_hi: Option<f64>,
    /// Rig DSP-function states over CAT. `None` = the rig doesn't report the func (hide its
    /// toggle); `Some(bool)` = supported + current on/off. Same `None = can't do it` idiom as
    /// `smeter_db`. `notch` is Hamlib's auto-notch (ANF), the one useful as a bare SSB toggle.
    #[serde(default)]
    pub nb: Option<bool>,
    #[serde(default)]
    pub nr: Option<bool>,
    #[serde(default)]
    pub notch: Option<bool>,
    #[serde(default)]
    pub comp: Option<bool>,
    #[serde(default)]
    pub vox: Option<bool>,
    /// The rig's BUILT-IN ANTENNA TUNER (Hamlib `RIG_FUNC_TUNER`): `None` = the radio doesn't
    /// report one, so no ATU control is offered at all; `Some(bool)` = it has one, and the bool is
    /// whether the tuner is currently switched in-line. Same `None = can't do it` idiom as the DSP
    /// funcs above — but NOT one of them: running the tuner keys the transmitter, so it rides its
    /// own gated command (`atu_tune`), never the generic `set_rig_func`.
    #[serde(default)]
    pub atu: Option<bool>,
    /// Rig RX passband / filter width in Hz from CAT; `None` = unknown or the rig's own default.
    #[serde(default)]
    pub filter_width_hz: Option<u32>,
    /// RIT (receive incremental tuning) offset in Hz — last commanded (0 = off). Optimistic.
    #[serde(default)]
    pub rit_hz: i32,
    /// XIT (transmit incremental tuning) offset in Hz — last commanded (0 = off). Optimistic.
    #[serde(default)]
    pub xit_hz: i32,
    /// Active VFO ("A" / "B") — last commanded. Optimistic (no read-back).
    #[serde(default)]
    pub active_vfo: String,
    /// RX input audio level (0.0–1.0), a decaying peak meter for the UI.
    #[serde(default)]
    pub rx_level: f32,
    /// Whether normal slot TX is enabled. False = Monitor-off (operator muted
    /// transmit); the engine produces no TX waveforms while this is false.
    #[serde(default = "default_true")]
    pub tx_enabled: bool,
    /// Whether the operator's license class permits transmitting at the current dial + mode.
    /// False = TX hard-blocked (outside privileges); the cockpit shows a lock indicator.
    /// Defaults true (Open / no-lockout) so an old snapshot never shows a phantom lock.
    #[serde(default = "default_true")]
    pub tx_allowed: bool,
    /// Whether the operator is holding a steady tune carrier (for ATU / amp
    /// tuning). While true the radio plays a continuous f0 sine instead of slots.
    #[serde(default)]
    pub tuning: bool,
    /// ⭐ WHO HOLDS THE TRANSMITTER, or `None` when nobody does — the UI's copy of
    /// [`Engine::tx_owner`], carrying its [`TxOwner::busy_reason`] verbatim.
    ///
    /// This exists because the UI was GUESSING. A "may I move the VFO?" gate written as
    /// `!transmitting && !tuning` is a partial reimplementation of the arbiter, and it knows
    /// only two of its seven owners: `transmitting` is written solely by the FT slot-TX path,
    /// so manual PTT, the voice keyer, CW, RTTY and SSTV are all invisible to it. That let the
    /// dial move under a held mic key (2026-08-05). The soundcard modes were covered by
    /// accident downstream via `tx_until_ms`; manual PTT is DELIBERATELY not
    /// (`tempo_audio::service` — "a section/mode change must always reach the rig"), so there
    /// is no backstop and the UI gate is the only thing standing there.
    ///
    /// ⚠️ NEW TX GATES IN THE UI READ THIS FIELD. Do not add another flag pair and re-derive
    /// the answer — that is the bug this field exists to end. `Some(_)` means the transmitter
    /// is busy AND carries the sentence to show the operator; there is nothing to look up.
    #[serde(default)]
    pub tx_busy_reason: Option<String>,
    /// Whether the transmit watchdog has tripped (continuous-TX limit reached)
    /// and auto-halted transmit. Cleared by re-enabling TX.
    #[serde(default)]
    pub tx_watchdog: bool,
    /// FT8/FT4 decode depth (1=Fast, 2=Normal, 3=Deep) — mirrored into the snapshot so the Operate
    /// cockpit can show + change it live (a mid-session CPU/battery lever), not only Settings.
    #[serde(default = "default_decode_depth_dto")]
    pub decode_depth: u8,
    /// Whether a QSO recording (audio bridge) is streaming live RX to disk. Drives the
    /// Phone cockpit's REC badge; persists across UI nav (it's loop-owned, not per-view).
    #[serde(default)]
    pub qso_recording: bool,
    /// Rig/CAT connection health: `None` = not applicable (VOX, no CAT),
    /// `Some(true)` = CAT connected (or serial port opened), `Some(false)` =
    /// CAT configured but failing. Drives the Test-CAT result + a status chip.
    #[serde(default)]
    pub cat_ok: Option<bool>,
    /// Human-readable rig/CAT status detail, e.g. "Connected — 14.074 MHz",
    /// "VOX — no CAT", or a specific error ("rigctld not reachable…").
    #[serde(default)]
    pub cat_detail: String,
    /// The radio's RECEIVE frequency ranges (MHz, inclusive `[lo, hi]` pairs) from Hamlib's
    /// capability table. **EMPTY = unknown, which means ALLOW** — no CAT, caps not probed, or a
    /// `\dump_state` we could not parse. Lets the UI avoid offering a "move the radio" control at
    /// a frequency the radio provably cannot reach (an HF-only rig and the 2 m APRS channel).
    #[serde(default)]
    pub rx_ranges_mhz: Vec<(f64, f64)>,
    /// The dial (MHz) the radio most recently REFUSED, so the UI can name it. `None` = none.
    #[serde(default)]
    pub refused_dial_mhz: Option<f64>,
    /// The AGC speed ("fast"|"mid"|"slow") the radio most recently REFUSED — Hamlib's AGC is an
    /// enum and backends do not all implement every step, MEDIUM least of all. The cockpit's
    /// segmented chip is optimistic (the rig read-back lags a poll), so without this it would
    /// keep claiming a speed the radio never took. `None` = the last AGC write was accepted.
    #[serde(default)]
    pub refused_agc: Option<String>,
    /// The CW keyer backend: "cat" (the rig generates CW → rig in CW mode) or "soundcard"
    /// (a keyed audio tone → rig deliberately in USB/LSB). Surfaced so the CW cockpit's
    /// toggle reflects the ACTUAL backend setting instead of a stale local default — that
    /// desync made CW land on USB when the persisted keyer was Soundcard.
    #[serde(default)]
    pub cw_keyer: String,
    /// The keyer speed (WPM) the engine is actually using — round-tripped so the CW
    /// cockpit's slider doesn't silently reset to 25 on every mount.
    #[serde(default)]
    pub cw_wpm: u32,
    /// Rig split: the TX dial (MHz) when split is configured (pile-up "UP n"
    /// spots), `None` = simplex. Drives the SPLIT badge.
    #[serde(default)]
    pub split_tx_mhz: Option<f64>,
    /// Set when the sound-card input/output failed to open, so the UI can show
    /// why the waterfall is blank instead of failing silently.
    #[serde(default)]
    pub audio_error: Option<String>,
    /// Set when two enabled radios are configured on the SAME serial COM port — the
    /// monitor radio's CAT can't open the busy port and shows a confusing red pill.
    /// A config warning (self-clears once the ports differ); surfaced in the status lane.
    #[serde(default)]
    pub radio_config_warning: Option<String>,
    /// The last per-QSO recording failed, with the full path it failed at. Surfaced in the status
    /// lane and cleared by the next recording that succeeds.
    ///
    /// A recording that fails silently is the same honesty defect as a feature that reports
    /// success while doing nothing — both the directory create and the WAV write used to discard
    /// their result, so a full disk or an unwritable profile dir produced no file and no word of
    /// it anywhere (#24).
    #[serde(default)]
    pub recording_warning: Option<String>,
    /// Transmit on even/"1st" slots (true) or odd/"2nd" slots (false). Two
    /// stations must use OPPOSITE periods to complete a QSO.
    #[serde(default = "default_true")]
    pub tx_even: bool,
    /// Smart auto-cycle on: answering a heard station auto-picks the opposite cycle
    /// (FT8-style). False = the operator fixed the cycle manually (Tx 1st/2nd).
    #[serde(default = "default_true")]
    pub tx_cycle_auto: bool,
    /// The active T/R period in seconds (FT1 = 4 s, FT8 = 15 s, FT4 = 7.5 s) — lets the
    /// UI label "1st/2nd" with the real period instead of assuming 15 s.
    #[serde(default)]
    pub tr_period_secs: f64,
    /// Heartbeat on: periodically announce presence (a low-cadence beacon) so listening
    /// stations enter each other's rosters and store-and-forward can deliver. Operator
    /// toggles it from the Tempo main screen.
    #[serde(default)]
    pub beacon: bool,
    /// Receive audio offset (Hz) — the green waterfall marker.
    #[serde(default = "default_offset")]
    pub rx_offset_hz: f32,
    /// Transmit audio offset (Hz) — the red waterfall marker.
    #[serde(default = "default_offset")]
    pub tx_offset_hz: f32,
    /// Keep TX offset fixed when RX changes (WSJT-X "Hold Tx Freq").
    #[serde(default)]
    pub hold_tx_freq: bool,
    /// Real PC-clock-vs-UTC offset in ms from an NTP probe, or `None` when the
    /// probe is disabled / offline (then the UI falls back to DT-derived health).
    #[serde(default)]
    pub clock_offset_ms: Option<i64>,
    /// Where decodes come from: the native engine or a WSJT-X/JTDX/MSHV companion.
    #[serde(default)]
    pub source: SourceKind,
    /// Human-readable source label, e.g. "Native (FT8)" or "WSJT-X UDP".
    #[serde(default)]
    pub source_label: String,
    /// TX audio drive level (0.0–1.0) — the "Pwr" slider; trim until ALC is ~zero.
    #[serde(default = "default_txlevel")]
    pub tx_level: f32,
    /// A CAT read from the rig succeeded this session — the dial/mode shown are the
    /// rig's own values, not the persisted seed (read-only launch provenance). False
    /// for VOX/serial-PTT rigs with no control channel, and after a CAT breaker trip.
    #[serde(default)]
    pub rig_confirmed: bool,
}

/// serde default helper: TX drive defaults to 0.9.
fn default_txlevel() -> f32 {
    0.9
}

/// serde default helper: `tx_enabled` defaults to true on partial deserialize.
fn default_true() -> bool {
    true
}
fn default_decode_depth_dto() -> u8 {
    3
}

/// serde default helper: audio offsets default to the 1500 Hz passband center.
fn default_offset() -> f32 {
    1500.0
}

/// One waterfall row: ~120 magnitudes in 0..1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spectrum {
    pub row: Vec<f32>,
    /// The window the row spans (Hz) — so the UI never hardcodes it. For the audio-FFT
    /// scope this is the audio passband (0–4000 Hz); for a native RF panadapter it's the
    /// absolute RF span. `f64` because an absolute VHF/UHF edge (e.g. 432.1 MHz) exceeds
    /// `f32`'s exact-integer range (2^24 ≈ 16.7 MHz).
    #[serde(default)]
    pub lo_hz: f64,
    #[serde(default)]
    pub hi_hz: f64,
    /// Where this row came from: `"audio"` (soundcard FFT), `"flex"` (SmartSDR VITA-49),
    /// or `"civ"` (Icom CI-V scope) — lets the UI label a native panadapter. Empty = audio.
    #[serde(default)]
    pub source: String,
}

/// The live meters (`get_meters`), read lock-free off `engine::MeterFeed` — the meter widgets
/// poll this fast (~100 ms) instead of riding the 300 ms snapshot, and it can never be frozen
/// by a CAT stall because no engine mutex is involved.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterReadout {
    /// RX input level (0..1 RMS, instrument ballistics applied) — the UI renders it as a
    /// WSJT-X-style dB level. 0.0 when no capture is open.
    pub rx_level: f32,
    /// CAT S-meter (dB relative to S9). `None` = the rig reports no STRENGTH (the meter shows
    /// "—" — absence stays absent, never a stale or invented level).
    pub smeter_db: Option<i32>,
}

/// The operating mode of the live engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpMode {
    /// Free-text directed messaging + presence (default).
    Chat,
    /// Auto-sequenced ragchew QSO (calling CQ or answering).
    Qso,
    /// ARRL Field Day exchange (running or search-and-pounce).
    FieldDay,
}

/// Status of an in-progress auto-sequenced QSO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QsoStatus {
    /// Sequencer state, e.g. "callingCq", "awaitReport", "done".
    pub state: String,
    pub dxcall: Option<String>,
    /// The DX's grid: what the exchange carried, else what we decoded from this station
    /// earlier in the session. Same resolution the logged GRIDSQUARE uses, so the cockpit's
    /// DX Grid box and the log can never disagree.
    #[serde(default)]
    pub dxgrid: Option<String>,
    /// Signal report received about my own signal, if any.
    pub rx_report: Option<i32>,
    /// True if this station is calling CQ (running) vs answering (S&P).
    pub running: bool,
    /// On-air text of the message queued for the next TX slot (the "Now sending"
    /// readout), or `None` when listening / the QSO is complete.
    #[serde(default)]
    pub tx_now: Option<String>,
    /// True when the current step has been retransmitted to its limit without the
    /// partner advancing — the sequencer is withholding further TX (operator may
    /// Resend or move on).
    #[serde(default)]
    pub stalled: bool,
    /// How many times the current message has been transmitted this step (resets
    /// when the partner advances the QSO) — the "I've called them N times" count.
    #[serde(default)]
    pub tx_count: u32,
}

/// Status of the coordinated-QSY ("move together") feature — a SEPARATE, opt-in
/// function. Present in the snapshot only while `qsy_enabled`; the UI renders its
/// own self-contained panel from it and otherwise ignores it (isolation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QsyStatus {
    /// Whether the feature is currently enabled.
    pub enabled: bool,
    /// Held on the current channel (manual pause).
    pub paused: bool,
    /// "initiator" (announces moves), "follower" (auto-follows), or "idle"
    /// (no partner selected → nothing to coordinate).
    pub role: String,
    /// The station we're roaming with, if selected.
    pub partner: Option<String>,
    /// Home channel token (where the conversation started).
    pub home: Option<String>,
    /// Channel token we're currently on.
    pub current: Option<String>,
    /// The next scheduled move's target channel token, if any (HOME = return home).
    pub next_channel: Option<String>,
    /// Absolute UTC slot the next move executes on, if scheduled.
    pub next_slot: Option<u64>,
    /// True after a "lost sync → home" fall-back fired.
    pub lost_sync: bool,
}

/// A single logged Field Day contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDayQso {
    pub call: String,
    pub class: String,
    pub section: String,
    pub band: String,
    /// Scoring class: "DIG" | "CW" | "PH".
    #[serde(default)]
    pub mode: String,
    /// Unix seconds when logged (drives interop-push timestamps).
    #[serde(default)]
    pub when_unix: u64,
}

/// Field Day mode status: my exchange, the log, score and multipliers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDayStatus {
    pub my_class: String,
    pub my_section: String,
    pub running: bool,
    pub state: String,
    /// The station currently being worked (the FD sequencer's partner) — lets
    /// the UI quiet decode popups about them mid-contact, like QsoStatus.dxcall.
    #[serde(default)]
    pub dxcall: Option<String>,
    pub qso_count: usize,
    pub sections: usize,
    /// The distinct sections worked (the identities behind `sections`), sorted —
    /// the worked-sections color board (spec §5) reads this.
    #[serde(default)]
    pub worked_sections: Vec<String>,
    /// Raw per-mode QSO points (phone 1, CW/digital 2) before multipliers.
    pub points: u32,
    /// Which event: "arrlfd" | "wfd".
    #[serde(default)]
    pub event: String,
    /// QSO points × the power multiplier (the submittable QSO score).
    #[serde(default)]
    pub powered_points: u32,
    /// Claimed bonus points (the Settings checklist).
    #[serde(default)]
    pub bonus_points: u32,
    /// powered_points + bonus_points — the claimed total.
    #[serde(default)]
    pub total_score: u32,
    pub log: Vec<FieldDayQso>,
}

/// Serializable per-source upload status (mirror of `tempo_core` `UploadStatus`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadStatusDto {
    /// "pending" | "accepted" | "duplicate" | "rejected" | "authfail".
    pub outcome: String,
    pub when_unix: i64,
    pub detail: Option<String>,
}

/// Serializable per-source outbound upload state (mirror of `UploadState`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadStateDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lotw: Option<UploadStatusDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eqsl: Option<UploadStatusDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qrz: Option<UploadStatusDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clublog: Option<UploadStatusDto>,
}

impl From<tempo_core::logbook::UploadStatus> for UploadStatusDto {
    fn from(s: tempo_core::logbook::UploadStatus) -> Self {
        UploadStatusDto {
            outcome: s.outcome.code().to_string(),
            when_unix: s.when_unix,
            detail: s.detail,
        }
    }
}
impl From<UploadStatusDto> for tempo_core::logbook::UploadStatus {
    fn from(s: UploadStatusDto) -> Self {
        tempo_core::logbook::UploadStatus {
            outcome: tempo_core::logbook::UploadOutcome::from_code(&s.outcome)
                .unwrap_or(tempo_core::logbook::UploadOutcome::Rejected),
            when_unix: s.when_unix,
            detail: s.detail,
        }
    }
}
impl From<tempo_core::logbook::UploadState> for UploadStateDto {
    fn from(u: tempo_core::logbook::UploadState) -> Self {
        UploadStateDto {
            lotw: u.lotw.map(Into::into),
            eqsl: u.eqsl.map(Into::into),
            qrz: u.qrz.map(Into::into),
            clublog: u.clublog.map(Into::into),
        }
    }
}
impl From<UploadStateDto> for tempo_core::logbook::UploadState {
    fn from(u: UploadStateDto) -> Self {
        tempo_core::logbook::UploadState {
            lotw: u.lotw.map(Into::into),
            eqsl: u.eqsl.map(Into::into),
            qrz: u.qrz.map(Into::into),
            clublog: u.clublog.map(Into::into),
        }
    }
}

/// A single logged contact from the general logbook (Chat/QSO contacts; Field
/// Day keeps its own contest log). The serializable mirror of
/// `tempo_core::logbook::QsoRecord`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedQso {
    pub call: String,
    pub grid: Option<String>,
    /// DXCC entity name (country), resolved from the callsign — the key DXer field.
    #[serde(default)]
    pub country: Option<String>,
    /// The cty.dat-RESOLVED entity for this row's callsign, filled by the
    /// command layer (which owns the table). THE award identity for UI
    /// comparisons: the stored `country` is free text whose spelling depends on
    /// who wrote it (QRZ says "Germany"/"Russia", cty.dat says "Fed. Rep. of
    /// Germany"/"European Russia"), so keying "new one" or entity counts on it
    /// made the two systems disagree forever. `country` is display text.
    #[serde(default)]
    pub entity: Option<String>,
    /// US state (ADIF STATE, 2-letter) for WAS, when known.
    #[serde(default)]
    pub state: Option<String>,
    pub band: String,
    pub freq_mhz: f64,
    /// Mode / tier label ("TempoFast" | "FT8" | "CW" | "SSB" | "USB" | "LSB" | "FM" …).
    pub mode: String,
    /// Signal report sent / received as a string: CW "599" / phone "59" / digital "-12".
    pub rst_sent: Option<String>,
    pub rst_rcvd: Option<String>,
    /// Operator name (ADIF NAME) — callbook autofill / ragchew logging.
    #[serde(default)]
    pub name: Option<String>,
    /// QSO location / city (ADIF QTH).
    #[serde(default)]
    pub qth: Option<String>,
    /// Short sharable remark (ADIF COMMENT).
    #[serde(default)]
    pub comment: Option<String>,
    /// Free-form multi-line operator notes (ADIF NOTES).
    #[serde(default)]
    pub notes: Option<String>,
    /// Transmit power in watts (ADIF TX_PWR).
    #[serde(default)]
    pub tx_power: Option<f64>,
    /// Contact time, Unix seconds (UTC).
    pub when_unix: u64,
    /// Whether the time of day is actually KNOWN. `false` for imported
    /// date-only records: `when_unix` then anchors at midnight for ordering,
    /// no TIME_ON is written, and LoTW/eQSL sends exclude the record (both
    /// match on time). Defaults `true` so legacy payloads keep their meaning.
    #[serde(default = "default_true")]
    pub time_known: bool,
    /// Confirmed via ANY channel (LoTW / eQSL / paper QSL).
    pub confirmed: bool,
    /// Award-eligible confirmation (LoTW or paper only — eQSL excluded). Drives
    /// the award counts; the UI can distinguish award-grade from eQSL-only.
    #[serde(default)]
    pub award_confirmed: bool,
    /// WHICH channel(s) confirmed — the per-source truth behind the booleans
    /// (all-false on legacy records whose sync predates the split).
    #[serde(default)]
    pub qsl_rcvd: QslRcvdDto,
    /// Operator-declared OUTBOUND QSL-request state (did I send a card, how, when).
    /// A request, NOT a confirmation.
    #[serde(default)]
    pub qsl_sent: QslSentDto,
    /// Awards credit GRANTED by ARRL (normalized ADIF codes, e.g. "DXCC").
    #[serde(default)]
    pub credit_granted: Vec<String>,
    /// Awards credit applied/submitted but not yet granted.
    #[serde(default)]
    pub credit_submitted: Vec<String>,
    /// Per-source outbound upload state (drives the "Upload to LoTW (N)" count +
    /// the diagnostics R1/R9/R2 reasons).
    #[serde(default)]
    pub upload: UploadStateDto,
    /// POTA/SOTA on-the-air references — my activation (my_*) and the station I hunted (their_*).
    /// Previously dropped from the DTO, so parks were invisible to the log form + table.
    #[serde(default)]
    pub ota: OtaDto,
    /// Import-carried award identity + the unmodelled-field remainder (see
    /// `QsoRecord`). Carried through the DTO so a per-row connector push and an
    /// edit round-trip don't strip a satellite QSO's credit fields or a master
    /// log's foreign fields.
    #[serde(default)]
    pub dxcc: Option<u32>,
    #[serde(default)]
    pub prop_mode: Option<String>,
    #[serde(default)]
    pub sat_name: Option<String>,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub station_callsign: Option<String>,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
}

/// On-the-air (POTA/SOTA) references for a QSO — serde mirror of `tempo_core::logbook::Ota`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtaDto {
    /// The program I'm activating under ("POTA" / "SOTA"), if activating.
    pub my_program: Option<String>,
    /// My park/summit reference ("K-1234" / "W7A/MN-001").
    pub my_ref: Option<String>,
    /// The program of the station I hunted.
    pub their_program: Option<String>,
    /// The park/summit reference I hunted.
    pub their_ref: Option<String>,
    /// The worked station's IOTA island-group reference ("NA-001").
    pub iota: Option<String>,
}

impl From<tempo_core::logbook::Ota> for OtaDto {
    fn from(o: tempo_core::logbook::Ota) -> Self {
        OtaDto {
            my_program: o.my_program,
            my_ref: o.my_ref,
            their_program: o.their_program,
            their_ref: o.their_ref,
            iota: o.iota,
        }
    }
}

impl From<OtaDto> for tempo_core::logbook::Ota {
    fn from(o: OtaDto) -> Self {
        tempo_core::logbook::Ota {
            my_program: o.my_program,
            my_ref: o.my_ref,
            their_program: o.their_program,
            their_ref: o.their_ref,
            iota: o.iota,
        }
    }
}

impl From<tempo_core::logbook::QsoRecord> for LoggedQso {
    fn from(r: tempo_core::logbook::QsoRecord) -> Self {
        LoggedQso {
            call: r.call,
            grid: r.grid,
            country: r.country,
            // Filled by the command layer (get_log) — tempo-app has no cty.dat.
            entity: None,
            state: r.state,
            band: r.band,
            freq_mhz: r.freq_mhz,
            mode: r.mode,
            rst_sent: r.rst_sent,
            rst_rcvd: r.rst_rcvd,
            name: r.name,
            qth: r.qth,
            comment: r.comment,
            notes: r.notes,
            tx_power: r.tx_power,
            when_unix: r.when_unix,
            time_known: r.time_known,
            confirmed: r.confirmed,
            award_confirmed: r.award_confirmed,
            qsl_rcvd: QslRcvdDto {
                card: r.qsl_rcvd.card,
                lotw: r.qsl_rcvd.lotw,
                eqsl: r.qsl_rcvd.eqsl,
                qrz: r.qsl_rcvd.qrz,
            },
            qsl_sent: r.qsl_sent.into(),
            credit_granted: r.credit_granted,
            credit_submitted: r.credit_submitted,
            upload: r.upload.into(),
            ota: r.ota.into(),
            dxcc: r.dxcc,
            prop_mode: r.prop_mode,
            sat_name: r.sat_name,
            operator: r.operator,
            station_callsign: r.station_callsign,
            extra: r.extra,
        }
    }
}

/// Per-channel inbound confirmation (mirrors tempo_core::logbook::QslRcvd).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QslRcvdDto {
    pub card: bool,
    pub lotw: bool,
    pub eqsl: bool,
    /// QRZ Logbook native confirmation — displayed as confirmed, but not award-eligible.
    #[serde(default)]
    pub qrz: bool,
}

/// Operator-declared OUTBOUND QSL-request state (mirrors tempo_core::logbook::QslSent).
/// A request, NOT a confirmation. `via` is the ADIF `QSL_SENT_VIA` letter ("B"/"D"/"E").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QslSentDto {
    pub sent: bool,
    pub via: Option<char>,
    pub date_unix: Option<u64>,
}

impl From<tempo_core::logbook::QslSent> for QslSentDto {
    fn from(s: tempo_core::logbook::QslSent) -> Self {
        QslSentDto {
            sent: s.sent,
            via: s.via.map(|v| v.code().chars().next().unwrap_or('?')),
            date_unix: s.date_unix,
        }
    }
}
impl From<QslSentDto> for tempo_core::logbook::QslSent {
    fn from(s: QslSentDto) -> Self {
        tempo_core::logbook::QslSent {
            sent: s.sent,
            via: s
                .via
                .and_then(|c| tempo_core::logbook::QslVia::from_code(&c.to_string())),
            date_unix: s.date_unix,
        }
    }
}

impl From<LoggedQso> for tempo_core::logbook::QsoRecord {
    fn from(q: LoggedQso) -> Self {
        tempo_core::logbook::QsoRecord {
            call: q.call,
            grid: q.grid,
            country: q.country,
            state: q.state,
            band: q.band,
            freq_mhz: q.freq_mhz,
            mode: q.mode,
            rst_sent: q.rst_sent,
            rst_rcvd: q.rst_rcvd,
            name: q.name,
            qth: q.qth,
            comment: q.comment,
            notes: q.notes,
            tx_power: q.tx_power,
            when_unix: q.when_unix,
            time_off_unix: None, // not carried on the DTO; set at log time / via ADIF import
            confirmed: q.confirmed,
            award_confirmed: q.award_confirmed,
            qsl_rcvd: tempo_core::logbook::QslRcvd {
                card: q.qsl_rcvd.card,
                lotw: q.qsl_rcvd.lotw,
                eqsl: q.qsl_rcvd.eqsl,
                qrz: q.qsl_rcvd.qrz,
            },
            qsl_sent: q.qsl_sent.into(),
            credit_granted: q.credit_granted,
            credit_submitted: q.credit_submitted,
            upload: q.upload.into(),
            ota: q.ota.into(),
            // Carried through the DTO round trip (a UI edit of an imported
            // date-only record must not fabricate time-knowledge). update_record
            // additionally guards the unchanged-time-of-day case.
            time_known: q.time_known,
            dxcc: q.dxcc,
            prop_mode: q.prop_mode,
            sat_name: q.sat_name,
            operator: q.operator,
            station_callsign: q.station_callsign,
            extra: q.extra,
        }
    }
}

/// Result of a LoTW upload attempt (a TQSL batch sign+upload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadReportDto {
    /// QSOs in the batch dispatched to TQSL.
    pub dispatched: usize,
    /// Outcome tag (lowercase): "pending" (signed+sent) | "duplicate" (all already
    /// on file) | "rejected" | "authfail" | "retry" (network — try again) | "none"
    /// (nothing to upload).
    pub outcome: String,
    /// Sanitized TQSL message on a non-success outcome.
    pub detail: Option<String>,
}

/// Result of importing an external ADIF logbook (deduped merge).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStats {
    pub added: usize,
    pub skipped: usize,
    /// Records ALREADY in the log that this import upgraded — the confirmations,
    /// credits and STATE/COUNTRY detail a LoTW/eQSL/QRZ download restates about
    /// contacts you logged long ago. Counted separately from `added` because it is
    /// invisible in the QSO count: an import that adds nothing can still be the one
    /// that fixes every award.
    pub updated: usize,
    pub total: usize,
}

/// A confirmation in a synced report with no matching logged QSO (diagnostic).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LotwOrphan {
    pub call: String,
    pub band: String,
    pub mode: String,
    pub when_unix: u64,
    pub reason: String,
}

/// Result of reconciling a LoTW (or any ADIF) confirmation report into the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LotwSyncResult {
    pub matched: usize,
    pub newly_confirmed: usize,
    /// Newly confirmed by ANY channel (incl. eQSL) — the headline count for an
    /// eQSL sync, where `newly_confirmed` (award-grade) is always 0.
    pub newly_confirmed_any: usize,
    pub newly_credited: usize,
    pub newly_submitted: usize,
    /// QSOs whose own LoTW upload was promoted Pending→Accepted by the own-echo
    /// pull this sync (your side is now confirmed on file). 0 for a paste-reconcile.
    pub promoted: usize,
    /// New QSOs pulled DOWN and added to the log (QRZ two-way sync only — LoTW/eQSL
    /// reconcile confirmations onto existing QSOs and never add). 0 for those paths.
    #[serde(default)]
    pub added: usize,
    pub orphans: Vec<LotwOrphan>,
}

impl From<tempo_core::reconcile::ReconcileSummary> for LotwSyncResult {
    fn from(s: tempo_core::reconcile::ReconcileSummary) -> Self {
        LotwSyncResult {
            matched: s.matched,
            newly_confirmed: s.newly_confirmed,
            newly_confirmed_any: s.newly_confirmed_any,
            newly_credited: s.newly_credited,
            newly_submitted: s.newly_submitted,
            promoted: 0, // set by the online sync after the own-echo pull
            added: 0,    // set by the QRZ two-way sync after the import pass

            orphans: s
                .orphans
                .into_iter()
                .map(|o| LotwOrphan {
                    call: o.call,
                    band: o.band,
                    mode: o.mode,
                    when_unix: o.when_unix,
                    reason: o.reason,
                })
                .collect(),
        }
    }
}

// --- Confirmation diagnostics ("why isn't this QSO confirmed?") ---------------

/// A structured, operator-facing action, flattened for the UI (only the fields
/// relevant to `kind` are populated).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDto {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logged: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_unix: Option<i64>,
}

impl From<tempo_core::diagnostics::Action> for ActionDto {
    fn from(a: tempo_core::diagnostics::Action) -> Self {
        use tempo_core::diagnostics::Action as A;
        let mut d = ActionDto {
            kind: String::new(),
            source: None,
            detail: None,
            field: None,
            found: None,
            expected: None,
            logged: None,
            suggested: None,
            call: None,
            other_index: None,
            until_unix: None,
        };
        match a {
            A::UploadToLotw => d.kind = "uploadToLotw".into(),
            A::UploadToQrz => d.kind = "uploadToQrz".into(),
            A::UploadToEqsl => d.kind = "uploadToEqsl".into(),
            A::UploadToClublog => d.kind = "uploadToClublog".into(),
            A::ReUpload { source, detail } => {
                d.kind = "reUpload".into();
                d.source = Some(source);
                d.detail = detail;
            }
            A::Reauthenticate { source } => {
                d.kind = "reauthenticate".into();
                d.source = Some(source);
            }
            A::NudgePartner { call, source } => {
                d.kind = "nudgePartner".into();
                d.call = Some(call);
                d.source = Some(source);
            }
            A::FixField {
                field,
                found,
                expected,
            } => {
                d.kind = "fixField".into();
                d.field = Some(field);
                d.found = Some(found);
                d.expected = Some(expected);
            }
            A::CorrectBustedCall { logged, suggested } => {
                d.kind = "correctBustedCall".into();
                d.logged = Some(logged);
                d.suggested = Some(suggested);
            }
            A::MergeDuplicate { other_index } => {
                d.kind = "mergeDuplicate".into();
                d.other_index = Some(other_index);
            }
            A::Wait { until_unix } => {
                d.kind = "wait".into();
                d.until_unix = Some(until_unix);
            }
            A::None => d.kind = "none".into(),
        }
        d
    }
}

/// One ranked reason a QSO isn't confirmed (+ the suggested fix).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonDto {
    pub code: String,
    pub confidence: String,
    pub explanation: String,
    pub action: ActionDto,
}

fn reason_code_str(c: tempo_core::diagnostics::ReasonCode) -> &'static str {
    use tempo_core::diagnostics::ReasonCode as R;
    match c {
        R::R1NeverUploaded => "r1",
        R::R2PartnerHasnt => "r2",
        R::R3WrongSource => "r3",
        R::R4aBandMismatch => "r4a",
        R::R4bModeMismatch => "r4b",
        R::R4cDateMismatch => "r4c",
        R::R4dMissingState => "r4d",
        R::R5Lag => "r5",
        R::R6BustedCall => "r6",
        R::R7Duplicate => "r7",
        R::R9UploadBounced => "r9",
    }
}

impl From<tempo_core::diagnostics::Reason> for ReasonDto {
    fn from(r: tempo_core::diagnostics::Reason) -> Self {
        use tempo_core::diagnostics::Confidence as C;
        ReasonDto {
            code: reason_code_str(r.code).into(),
            confidence: match r.confidence {
                C::Confident => "confident".into(),
                C::Likely => "likely".into(),
            },
            explanation: r.explanation,
            action: r.action.into(),
        }
    }
}

/// A per-QSO diagnosis row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QsoDiagnosisDto {
    pub index: usize,
    pub award: String,
    pub status: String,
    pub reasons: Vec<ReasonDto>,
}

impl From<tempo_core::diagnostics::QsoDiagnosis> for QsoDiagnosisDto {
    fn from(d: tempo_core::diagnostics::QsoDiagnosis) -> Self {
        use tempo_core::diagnostics::QsoAwardStatus as S;
        QsoDiagnosisDto {
            index: d.index,
            award: d.award,
            status: match d.status {
                S::Credited => "credited".into(),
                S::Confirmed => "confirmed".into(),
                S::ConfirmedWrongSource => "confirmedWrongSource".into(),
                S::NeedsAction => "needsAction".into(),
                S::PendingLag => "pendingLag".into(),
            },
            reasons: d.reasons.into_iter().map(ReasonDto::from).collect(),
        }
    }
}

/// A rollup bucket ("12 QSOs need a LoTW confirmation").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionBucketDto {
    pub kind: String,
    pub count: usize,
    pub qso_indices: Vec<usize>,
}

/// One entity a single award-grade fix away from a new slot / new entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OneAwayDto {
    pub entity: String,
    pub bands: Vec<String>,
    pub new_entity: bool,
}

/// The whole confirmation-diagnostics report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReportDto {
    pub diagnoses: Vec<QsoDiagnosisDto>,
    pub buckets: Vec<ActionBucketDto>,
    #[serde(default)]
    pub one_away: Vec<OneAwayDto>,
    pub waiting_on_partner: usize,
    pub pending_lag: usize,
}

impl From<tempo_core::diagnostics::DiagnosticsReport> for DiagnosticsReportDto {
    fn from(r: tempo_core::diagnostics::DiagnosticsReport) -> Self {
        DiagnosticsReportDto {
            diagnoses: r.diagnoses.into_iter().map(QsoDiagnosisDto::from).collect(),
            buckets: r
                .buckets
                .into_iter()
                .map(|b| ActionBucketDto {
                    kind: b.kind,
                    count: b.count,
                    qso_indices: b.qso_indices,
                })
                .collect(),
            one_away: r
                .one_away
                .into_iter()
                .map(|o| OneAwayDto {
                    entity: o.entity,
                    bands: o.bands,
                    new_entity: o.new_entity,
                })
                .collect(),
            waiting_on_partner: r.waiting_on_partner,
            pending_lag: r.pending_lag,
        }
    }
}

/// A QRZ.com callsign-lookup result (the serde DTO over the pure
/// [`tempo_core::qrz::QrzLookup`]). `grid`/`state` are subscriber-only and are
/// routinely `None` for free QRZ accounts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrzLookupDto {
    pub call: String,
    pub name: Option<String>,
    /// QRZ `<nickname>` — the operator's preferred short name; the UI shows it in
    /// place of the full name when present. `None` for HamQTH (no such field).
    pub nickname: Option<String>,
    pub qth: Option<String>,
    pub grid: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    pub dxcc: Option<u32>,
    pub cq_zone: Option<u32>,
    pub itu_zone: Option<u32>,
    /// Profile photo URL (QRZ `<image>` / HamQTH `<picture>`) — routinely `None`.
    pub image: Option<String>,
    /// The station's EXACT position when the callbook reported a trustworthy one, so
    /// the caller card can show the same distance/bearing QRZ itself does instead of
    /// re-deriving them from the center of a grid square. Always both or neither.
    /// `None` is the normal case (free QRZ account, `<geoloc>` provenance too weak) and
    /// means "fall back to the locator" — never "0, 0".
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
}

impl From<tempo_core::qrz::QrzLookup> for QrzLookupDto {
    fn from(r: tempo_core::qrz::QrzLookup) -> Self {
        QrzLookupDto {
            call: r.call,
            name: r.name,
            nickname: r.nickname,
            qth: r.qth,
            grid: r.grid,
            state: r.state,
            country: r.country,
            dxcc: r.dxcc,
            cq_zone: r.cq_zone,
            itu_zone: r.itu_zone,
            image: r.image,
            lat: r.lat,
            lon: r.lon,
        }
    }
}

/// The free HamQTH fallback flows into the SAME DTO — its [`HamQthLookup`] has
/// identical fields, so QRZ and HamQTH results are interchangeable to the UI.
///
/// [`HamQthLookup`]: tempo_core::hamqth::HamQthLookup
impl From<tempo_core::hamqth::HamQthLookup> for QrzLookupDto {
    fn from(r: tempo_core::hamqth::HamQthLookup) -> Self {
        QrzLookupDto {
            call: r.call,
            name: r.name,
            nickname: None, // HamQTH has no nickname field
            qth: r.qth,
            grid: r.grid,
            state: r.state,
            country: r.country,
            dxcc: r.dxcc,
            cq_zone: r.cq_zone,
            itu_zone: r.itu_zone,
            image: r.image,
            lat: r.lat,
            lon: r.lon,
        }
    }
}

/// Result of a QRZ Logbook push (one-QSO INSERT). `result` is a camelCase tag the
/// UI switches on; a `duplicate` is the benign "already in your QRZ logbook".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrzPushResultDto {
    /// "ok" | "replace" | "duplicate" | "authFail" | "fail".
    pub result: String,
    pub logid: Option<String>,
    pub reason: Option<String>,
}

impl From<tempo_core::qrz::QrzPush> for QrzPushResultDto {
    fn from(p: tempo_core::qrz::QrzPush) -> Self {
        use tempo_core::qrz::QrzPushResult::*;
        let result = match p.result {
            Ok => "ok",
            Replace => "replace",
            Duplicate => "duplicate",
            AuthFail => "authFail",
            Fail => "fail",
        }
        .to_string();
        QrzPushResultDto {
            result,
            logid: p.logid,
            reason: p.reason,
        }
    }
}

/// Result of a ClubLog realtime push (one-QSO upload). `result` is a camelCase
/// outcome tag the UI switches on; `duplicate` is the benign "already on ClubLog".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClubLogPushResultDto {
    /// "ok" | "modified" | "duplicate" | "rejected" | "authFail" | "serverError" | "unknown".
    pub result: String,
    pub message: Option<String>,
}

impl From<tempo_core::clublog::ClubLogPush> for ClubLogPushResultDto {
    fn from(p: tempo_core::clublog::ClubLogPush) -> Self {
        use tempo_core::clublog::ClubLogResult::*;
        let result = match p.result {
            Ok => "ok",
            Modified => "modified",
            Duplicate => "duplicate",
            Rejected => "rejected",
            AuthFail => "authFail",
            ServerError => "serverError",
            Unknown => "unknown",
        }
        .to_string();
        ClubLogPushResultDto {
            result,
            message: p.message,
        }
    }
}

/// Result of an HRDLog.net upload (one-QSO `NewEntry.aspx`). `result` is a
/// camelCase outcome tag the UI switches on; `duplicate` is the benign "already on
/// HRDLog". HRDLog.net is a live-logging/awards site — never DXCC/WAS credit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HrdLogPushResultDto {
    /// "ok" | "duplicate" | "authFail" | "rejected" | "unknown".
    pub result: String,
    pub message: Option<String>,
}

impl From<tempo_core::hrdlog::HrdLogPush> for HrdLogPushResultDto {
    fn from(p: tempo_core::hrdlog::HrdLogPush) -> Self {
        use tempo_core::hrdlog::HrdLogResult::*;
        let result = match p.result {
            Ok => "ok",
            Duplicate => "duplicate",
            AuthFail => "authFail",
            Rejected => "rejected",
            Unknown => "unknown",
        }
        .to_string();
        HrdLogPushResultDto {
            result,
            message: p.message,
        }
    }
}

/// One saved SSTV image in the operator-browsable local gallery. The session
/// list lives on the engine; the decode thread persists it as `gallery.json`
/// beside the images in the `sstv-gallery/` folder (atomic tmp+rename, the
/// `openings_log.json` pattern).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SstvGalleryEntry {
    /// Absolute path of the saved image file (24-bit BMP).
    pub path: String,
    /// Mode label, e.g. "Scottie 1".
    pub mode: String,
    /// ISO-8601 UTC completion time, e.g. "2026-07-17T15:30:00Z".
    pub finished_utc: String,
    /// The dial frequency (MHz) when the image finished.
    pub freq_mhz: f64,
    /// Decoded scan lines in the finished image (= image height).
    pub lines: u32,
    /// The FSK callsign ID trailing the image (slowrx `fsk.c`), if a plausible
    /// one was recovered — else `None`. Optional: pre-FSK `gallery.json` files
    /// load this as `None` via the struct-level `default`.
    #[serde(default)]
    pub fsk_id: Option<String>,
}

/// AI CW decoder state for the CW cockpit (beta side panel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiCwStatus {
    pub enabled: bool,
    /// "listening…" / "model not installed" / "" (decoding normally).
    pub status: String,
    /// The rolling stitched transcript (overlapping windows deduplicated by time).
    pub text: String,
}

/// The full application snapshot the UI renders from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub mycall: String,
    pub mygrid: String,
    pub mode: OpMode,
    pub radio: RadioStatus,
    /// Multi-radio switcher summaries (dual-radio). One per configured radio; empty or 1-element
    /// for a single-radio station (the UI then shows no switcher). `radio` above is the active one.
    #[serde(default)]
    pub radios: Vec<RadioSummary>,
    /// The id of the active radio (matches one of `radios`).
    #[serde(default)]
    pub active_radio_id: u32,
    /// Peg-lock state (band selection won't auto-switch when true).
    #[serde(default)]
    pub radio_pegged: bool,
    /// AI CW decoder (beta): enabled flag, status line, recent window decodes.
    #[serde(default)]
    pub ai_cw: AiCwStatus,
    pub link: LinkState,
    /// Chat CQ run state: "off" | "calling" | "paused" (the Tempo keep-calling loop).
    #[serde(default)]
    pub chat_cq: String,
    pub stations: Vec<Station>,
    pub conversations: Vec<Conversation>,
    pub active_peer: Option<String>,
    /// Present when `mode == Qso`.
    pub qso: Option<QsoStatus>,
    /// Present when `mode == FieldDay`.
    pub field_day: Option<FieldDayStatus>,
    /// Signals decoded in the most recent RX slot (live decode feed).
    pub recent_decodes: Vec<DecodeRow>,
    /// JTAlert-style UDP callsign highlights (call → CSS colors) for the
    /// decode panes. Empty unless a cooperating app sent HighlightCallsign.
    #[serde(default)]
    pub highlights: Vec<HighlightEntry>,
    /// Bumped each time a spot is worked (work_spot) — the UI navigates to
    /// `work_view`'s cockpit on change, so a click in a pop-out window still
    /// lands the MAIN window in the right section (clearTick pattern).
    #[serde(default)]
    pub work_tick: u64,
    /// The last worked spot's mode: "digital" | "phone" | "cw".
    #[serde(default)]
    pub work_view: Option<String>,
    /// The last worked spot's callsign — a pop-out window's click prefills the MAIN
    /// window's log Call from this (cleared on a call-less work).
    #[serde(default)]
    pub work_call: Option<String>,
    /// Bumped by an inbound UDP Clear — the UI erases its panes on change.
    #[serde(default)]
    pub clear_tick: u32,
    /// Pending one-click POTA/SOTA hunt (the next QSO with this call auto-tags
    /// the park). None = not hunting.
    #[serde(default)]
    pub hunt: Option<HuntDto>,
    /// Coordinated-QSY status — present only while the (opt-in) feature is enabled.
    #[serde(default)]
    pub qsy: Option<QsyStatus>,
    /// Session count of IR-HARQ rescues (decodes recovered by combining
    /// retransmissions, rv > 0). For the HARQ stats readout.
    #[serde(default)]
    pub harq_rescues: u32,
    /// A completed QSO awaiting the operator's confirm-before-log (WSJT-X "Prompt
    /// me to log QSO"). Present only when `prompt_to_log` is on and a QSO just
    /// finished; the UI shows a confirm popup, then calls `confirm_pending_log` /
    /// `discard_pending_log`.
    #[serde(default)]
    pub pending_log: Option<LoggedQso>,
    /// Last connector auto-upload outcome (QRZ/ClubLog/eQSL) — operator-facing
    /// toast text; `upload_tick` bumps on each new outcome so the UI toasts it.
    #[serde(default)]
    pub upload_note: Option<String>,
    #[serde(default)]
    pub upload_ok: bool,
    #[serde(default)]
    pub upload_tick: u32,
}
