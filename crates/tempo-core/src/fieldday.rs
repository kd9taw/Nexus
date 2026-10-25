//! ARRL Field Day mode: the Class+Section exchange, an auto-sequencer that runs
//! operator-initiated two-way contacts, a dupe-checked log with scoring, and
//! ADIF / Cabrillo export.
//!
//! ⚠️ **Field Day has no section multiplier**, whatever this comment said until
//! 2026-09-08. The ARRL FD score is QSO points × the power tier plus claimed
//! bonuses — nothing in [`fd_rules`](crate::fd_rules) reads a section — and
//! [`FieldDayLog::sections`] is a display count, not a score input.
//!
//! Field Day requires operator-initiated contacts (no fully-automated QSOs), and
//! the exchange is **Class + ARRL/RAC Section** (e.g. `3A WI`). FT1 carries this
//! natively in one frame: `<to> <de> <class> <section>` (and the rogered
//! `<to> <de> R <class> <section>`).
//!
//! **Every other contest's FT8/FT4 messages are WSJT-X's for that contest, not Field
//! Day's** ([`FtMessages`]): the ARRL VHF contests call `CQ TEST` and exchange the grid, as
//! WSJT-X's NA VHF Contest mode does, and a contest WSJT-X has no mode for runs the standard
//! messages. Field Day's own messages, and whatever any tier but FT8 and FT4 sends, are
//! unchanged.

use crate::message::Msg;
use modes::Decode;
use std::collections::HashSet;

/// Which Field Day event is running — they share the exchange SHAPE
/// (designator + ARRL/RAC section) but differ in designator grammar, contest
/// ids, and scoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FdEvent {
    /// ARRL Field Day (4th full June weekend): class like `3A`, phone 1 pt,
    /// CW/digital 2 pts, power multiplier, ~100-pt bonus menu.
    #[default]
    ArrlFd,
    /// Winter Field Day (the fourth full January weekend): category like `2O`
    /// (count + Home/Indoor/Mobile/Outdoor).
    WinterFd,
}

impl FdEvent {
    pub fn contest_id(self) -> &'static str {
        match self {
            FdEvent::ArrlFd => "ARRL-FIELD-DAY",
            FdEvent::WinterFd => "WFD",
        }
    }
    /// The rules-file `event` id for this event — the inverse of
    /// [`from_code`](Self::from_code), and the key
    /// [`fd_rules::ruleset`](crate::fd_rules::ruleset) looks a ruleset up by.
    ///
    /// ⭐ A rules file's `event` is a plain id, not an `FdEvent`: the rules table
    /// carries contests this enum has no arm for (the state QSO parties), and giving
    /// each one an `FdEvent` arm would put every non-exhaustive match in the tree on
    /// the critical path of adding a contest. `FdEvent` stays what it is — the two
    /// Field Day events the app has dedicated behaviour for — and this is the one
    /// place the two vocabularies meet.
    pub fn code(self) -> &'static str {
        match self {
            FdEvent::ArrlFd => "arrlfd",
            FdEvent::WinterFd => "wfd",
        }
    }
    /// Every event this enum can name. The rules floor is derived from it (see
    /// [`fd_rules`](crate::fd_rules)), because these are exactly the events looked up
    /// through an INFALLIBLE accessor.
    pub const ALL: [FdEvent; 2] = [FdEvent::ArrlFd, FdEvent::WinterFd];
    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("wfd") {
            FdEvent::WinterFd
        } else {
            FdEvent::ArrlFd
        }
    }
    /// The inverse of [`contest_id`](Self::contest_id) — how a log recovers its event
    /// from the session it was opened with, so the two can never disagree.
    pub fn from_contest_id(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("WFD") {
            FdEvent::WinterFd
        } else {
            FdEvent::ArrlFd
        }
    }
}

/// Per-QSO points by operating mode class (both events: phone 1, CW/digital 2 —
/// the long-standing ARRL FD values; WFD currently matches for the base QSO
/// point, with its own multiplier system handled at the score layer).
pub fn qso_points_for_mode(mode: &str) -> u32 {
    match mode.to_ascii_uppercase().as_str() {
        "PH" | "PHONE" | "SSB" | "FM" => 1,
        _ => 2, // CW + digital
    }
}

/// ⭐ **A contact worked THROUGH A BIRD** — everything about a satellite contact that is
/// a property of the PASS rather than of where the radio happens to be standing now.
///
/// ⚠️ **This type exists because the contest log's band is the LOG's, not the caller's.**
/// Every other entry point stamps [`FieldDayLog::band`], which the engine re-syncs from
/// the radio at the top of each log call — right for a knob-QSY between contacts, and
/// wrong for a bird. An operator turning a rotator by hand writes the contact up when
/// their hands are free, which is after LOS with the radio already back on the HF run,
/// so a 70 cm pass went into the contest log — and out to N1MM and N3FJP — on 20 m.
///
/// **The DOWNLINK, on both counts**, which is the answer the rest of the build already
/// gives and must not be contradicted here: `Engine::log_frequencies` returns the
/// receive leg ALONE for a cross-band pair (a satellite contact is not a same-band
/// split and writes no `FREQ_RX`), and `Engine::log_qso`'s `PROP_MODE`/`SAT_NAME` stamp
/// admits a record only when its frequency sits in the held bird's DOWNLINK passband.
/// Two emitters disagreeing about what frequency a contact was worked on is a contact
/// the operator cannot reconcile afterwards.
///
/// Not [`doppler::Legs`](crate::doppler::Legs), which is the live up/down tuning pair —
/// this is one finished contact's record of where it happened.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SatLeg {
    /// The amateur band of the downlink (`"70cm"`), stamped verbatim onto
    /// [`LoggedQso::band`] — and therefore into the dupe key, so a station worked
    /// through the bird and again on the HF run is two contacts, not a dupe.
    pub band: String,
    /// The downlink frequency in Hz at the moment of the contact. `0` = not known, and
    /// the row falls back to the log's own dial exactly as an ordinary row does.
    pub down_hz: u64,
    /// ⭐ **WHICH BIRD, as the dupe key means it** — the satellite's own catalog name
    /// (`"SAUDISAT 1C (SO-50)"`), never the transponder's.
    ///
    /// Separate from [`name`](Self::name) because they answer different questions and
    /// one of them is often absent: `name` is the designator LoTW will accept, and is
    /// `None` for a bird it does not list, while a contact still has to be told apart
    /// from one through a different bird. ARRL Field Day: *"Satellite QSOs also count
    /// for regular QSO credit. Show them listed separately on the summary sheet as a
    /// separate 'band.'"* — so this is what
    /// [`DupeRule::satellite_is_a_band`](crate::contest::DupeRule::satellite_is_a_band)
    /// puts in the key's band position.
    ///
    /// The TRANSPONDER is deliberately not in it: ARRL's limit is per SATELLITE, and a
    /// bird with a voice repeater and a packet digipeater is still one bird.
    pub bird: String,
    /// ⭐ **A single-channel FM satellite** — an FM voice repeater or a packet
    /// digipeater, as against a linear transponder.
    ///
    /// ARRL Field Day: *"Stations are limited to one (1) completed QSO on any single
    /// channel FM satellite."* Read by
    /// [`DupeRule::fm_satellite_once`](crate::contest::DupeRule::fm_satellite_once),
    /// which drops the mode class out of the key for such a bird — on one channel there
    /// is no second mode to work the same station in. `false` for a linear transponder,
    /// where the sponsor explicitly allows the additional QSO.
    ///
    /// ⚠️ **It is the CALLER's classification, not one derived here.** A half-width of
    /// zero says "a channel", which a linear transponder whose catalogue record is
    /// missing its passband also says — and the direction that mistake fails in is the
    /// bad one, silently refusing a legal contact. The satellite catalogue answers it
    /// exactly (`Transmitter::is_transponder` + `downlink_class`), so the engine is told
    /// rather than left to guess.
    pub single_channel_fm: bool,
    /// ADIF `SAT_NAME` — the hyphenated designator LoTW/TQSL actually matches
    /// (`"SO-50"`, parsed out of `"SAUDISAT 1C (SO-50)"`), never a raw catalog label.
    ///
    /// `None` for a bird LoTW does not list, and then **no satellite pair is written at
    /// all**: TQSL validates `PROP_MODE`/`SAT_NAME` as a pair and hard-errors on a lone
    /// member, which through the `-a compliant` funnel wedges the whole upload batch as
    /// Rejected. The band above still applies — where the contact was worked is true
    /// whether or not LoTW has heard of the bird.
    pub name: Option<String>,
}

/// A logged contest contact.
///
/// ⭐ **Both sides of the exchange live HERE, per row, and that is the whole of the
/// mobile fix.** `class`/`section` used to be the received half and there was no sent
/// half at all: the log carried ONE `Exchange` and `cabrillo()` wrote it on every QSO
/// line, so an operator who changed county re-labelled every contact made before the
/// change. A datum belongs on the row if it can change while a session is open, and the
/// sent exchange, the issued serial and the role all can.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedQso {
    pub call: String,
    /// The exchange THEY sent me. For Field Day this is class + section, which is
    /// exactly what the `class`/`section` pair held.
    pub rx: Vec<crate::contest::FieldValue>,
    /// The exchange I sent THEM, **as sent on this contact** — including the serial
    /// that was issued to it. Copied from the session at log time and never re-read
    /// from it afterwards.
    pub tx: Vec<crate::contest::FieldValue>,
    /// The [`RoleSpec`](crate::contest::RoleSpec) id this contact was worked under
    /// (`""` for a symmetric contest, which is both Field Day events).
    ///
    /// Carried per row because crossing a state line changes which role I am, and a
    /// row worked before the crossing must keep the one it was worked under. A session
    /// refuses that transition rather than mixing two roles in one log (§3.3 ruling 3);
    /// this field is what makes a mixed log DETECTABLE instead of aspirational.
    pub role: String,
    /// The DXCC/WAE entity for [`call`](Self::call), resolved once at log time rather
    /// than at export time.
    ///
    /// ⭐ Filled from [`contest::resolve_call`](crate::contest::resolve_call), which the
    /// composition root installs over the cty.dat resolver in `crates/propagation` — a
    /// crate that depends on this one, so this crate can never call it directly. `None`
    /// in a build with no resolver installed, and for a call the country file cannot
    /// place. Read by [`MultSource::DxccEntity`](crate::contest::MultSource::DxccEntity)
    /// (CQ WW's country multiplier) and, with [`continent`](Self::continent), by
    /// [`PointsRule::ByRelation`](crate::contest::PointsRule::ByRelation).
    pub entity: Option<String>,
    /// The continent code for [`call`](Self::call) — `AF`/`AS`/`EU`/`NA`/`OC`/`SA` —
    /// resolved with [`entity`](Self::entity) and from the same answer, so the two can
    /// never come from different readings of the same call.
    pub continent: Option<String>,
    /// The CQ WPX prefix for [`call`](Self::call), resolved once at log time by
    /// [`wpx_prefix`](crate::contest::wpx_prefix) — the sponsor's own §V.C.1 rule, not a
    /// country-file lookup, so it is filled in every build. `None` for a row whose call
    /// cannot be read as one.
    pub prefix: Option<String>,
    pub band: String,
    /// The dial the contact was logged on, in kHz — `0` when not known (a row logged
    /// before this existed, a path with no dial). A contest's Cabrillo writes it in
    /// place of the band edge when it lies inside the row's own band: CQ WW RTTY X.1,
    /// *"Stations competing for World and Continent awards must provide accurate
    /// frequencies for all contacts in the log."*
    pub freq_khz: u32,
    /// ⭐ **The frequency actually RADIATED** — the dial with the sideband-signed TX audio
    /// offset applied, in MHz. `0.0` = not known.
    ///
    /// ⚠️ **NOT the same fact as [`freq_khz`](Self::freq_khz), and that is deliberate.**
    /// Cabrillo's convention is the VFO readout, which is what a sponsor's cross-check
    /// expects and what `cabrillo_with` writes. ADIF `FREQ` means the frequency the
    /// transmission was ON, which for any AFSK/digital mode is the dial plus the audio
    /// offset (minus it on LSB). Collapsing the two made every digital contest contact
    /// export at the bare dial — 14.074000 — while the SAME operator's ordinary FT8 rows
    /// carried dial + offset, so one contact was described two different ways.
    ///
    /// Stamped at log time from `Engine::log_frequencies`, which is the general log's own
    /// answer: ONE function decides what frequency a contact was worked on, because two
    /// emitters disagreeing about it is a contact the operator cannot reconcile afterwards.
    ///
    /// In **Hz**, deliberately: this struct derives `Eq` and is compared for dupe detection,
    /// and a float frequency in a compared row is an equality that depends on how it was
    /// computed. Hz is exact at every frequency an amateur radio reaches.
    pub on_air_hz: u64,
    /// The RECEIVE leg, in MHz, and `None` unless the contact was genuinely worked split on
    /// ONE band. ADIF `FREQ_RX` equal to `FREQ` is not a harmless duplicate — it is a claim
    /// that the QSO was worked split, carried into every logger the export reaches.
    ///
    /// ⚠️ A split contact used to export the LISTENING leg as `FREQ` with no `FREQ_RX`: the
    /// wrong frequency, stated confidently, in the file that goes to LoTW, QRZ and Club Log.
    pub freq_rx_hz: Option<u64>,
    /// Mode class for scoring + per-band-mode dupes: "DIG" | "CW" | "PH".
    pub mode: String,
    /// The ACTUAL on-air mode behind a scoring class (ADIF name, uppercase:
    /// "FT8", "RTTY", "SSTV"…). Empty = not recorded (legacy rows) — exports
    /// fall back to the historical class map. WFD bans the WSJT modes but
    /// allows RTTY/SSTV, so an export must never claim "FT8" for an RTTY QSO.
    ///
    /// ⭐ **"PH" carries one too** — `USB` / `LSB` / `FM` / `AM`, the phone mode the
    /// contact was worked on, because `PH` is a SCORING class and covers all four. Before
    /// this every contest phone contact exported as bare `SSB`: no row said upper or
    /// lower, and an FM contact claimed an emission that was never on the air. It is
    /// filled by `Engine::phone_on_air_mode`, whose ⛔ rule is the general log's own —
    /// **the sideband comes from the rig or not at all**, never from the band default.
    pub submode: String,
    pub slot: u64,
    /// Unix seconds when the contact was logged — Cabrillo requires a real
    /// `yyyy-mm-dd hhmm` per QSO line (the old `----------` placeholder would
    /// FAIL ARRL submission). 0 in legacy/test paths = falls back to the
    /// placeholder rather than inventing a date.
    pub when_unix: u64,
    /// Per-position monotonic sequence number — this contact's half of the
    /// club-sync QSO id `(position id, seq)`. Stamped by
    /// [`FieldDayLog::log_submode_at`] at log time (every entry point funnels
    /// there), journaled as `APP_NEXUS_QSEQ`, and restored by
    /// [`FieldDayLog::merge_adif`]. Deliberately clock-free: positions' clocks
    /// disagree at a generator-powered site, so ids never derive from time.
    /// 0 = never assigned (rows built by paths that predate sync).
    pub seq: u64,
    /// ⭐ **This contact duplicates one already in the log** — it is REPORTED to the
    /// sponsor and worth nothing. Every contest logs its dupes
    /// ([`DupeRule::log_dupes`](crate::contest::DupeRule::log_dupes), the operator's ruling
    /// of 2026-10-11), so a row of any contest, Field Day's included, can carry it.
    ///
    /// ⚠️ **It is a fact about the LOG, not about the contact**, which is why it is
    /// RE-DERIVED by [`merge_adif`](FieldDayLog::merge_adif) from the dupe index rather
    /// than journaled as a tag — the rule `restore_row` already applies to the serial
    /// run, for the reason spelled out there: a flag stored beside the history that
    /// determines it is a second source of truth for one fact, and the two come apart
    /// exactly where it matters. A journal whose first occurrence aged out restores its
    /// survivor as the real contact, which is the right answer and one a persisted tag
    /// would get wrong.
    ///
    /// ⚠️ **It must never reach an exported Cabrillo as a marker.** Neither sponsor
    /// asks for one, and CQ's own worked example writes the dupe as a plain `QSO:`
    /// line; `X-QSO` means "I want this contact EXCLUDED" and carries CQ's anti-abuse
    /// warning. This flag is for OUR scoring and display.
    pub dupe: bool,
    /// ⭐ **Worked through a bird** ([`SatLeg`]) — `None` on every terrestrial contact,
    /// which is all but a handful of rows in a lifetime.
    ///
    /// It is what stamped this row's [`band`](Self::band), [`freq_khz`](Self::freq_khz)
    /// and [`on_air_hz`](Self::on_air_hz) in place of the log's own, and it is what
    /// carries ADIF `PROP_MODE`/`SAT_NAME` into the journal, the submitted file and the
    /// general-log merge — so a satellite contact made during Field Day still earns LoTW
    /// satellite credit and still reaches Nexus's own Satellite VUCC and needs boards.
    pub sat: Option<SatLeg>,
    /// ⭐ **Who was at the key when this contact was logged** — the callsign in the Field Day
    /// operator setting at that moment, uppercase, journaled as ADIF `OPERATOR`. `""` when
    /// nobody was named, which is every single-op station: it is never filled with the
    /// station's own call, because the row already carries that and an `OPERATOR` nobody
    /// chose would be indistinguishable from one somebody did.
    ///
    /// A club rotates operators through one position, so the person belongs to the ROW, not
    /// the log: the Cabrillo `OPERATORS` header is everyone the rows name.
    pub operator: String,
    /// ⭐ **The call this contact was SENT under, when it is not the log's own** — `""` on
    /// every row of a position's own log, whose contacts all went out under
    /// [`FieldDayLog::mycall`], the call a Cabrillo QSO line writes for a row with none.
    ///
    /// One row has another: an ARRL Field Day GOTA station's, which *"must use a different
    /// callsign from the primary Field Day station"* (2026 rules, 4.1.1.1) and whose QSOs
    /// *"may be claimed for credit by its primary Field Day operation"* (4.1.1.5). Its
    /// contacts reach the host's club log, whose file is written under the host's call, so
    /// the club's rebuild stamps the GOTA call here ([`FieldDayLog::station_call`]). The
    /// ADIF writer names no station on any row, so it has nothing of this to carry, and no
    /// journal holds a row with one: a club's rebuilt log is never journaled.
    pub station_call: String,
    /// ⭐ **The call this station was on the air under when the contact was logged** —
    /// Settings' "Callsign on the air" at that moment, as [`FieldDayLog::on_air_call`] held it.
    /// `""` on a row logged before it was recorded, and on every row of a log nothing stamps
    /// (a club's rebuild).
    ///
    /// Club sync reads it: a club's log takes a contact only on the call the club is on the
    /// air under, so one logged on another call stays in this log alone
    /// (`Engine::fd_on_another_call` in tempo-app). It is journaled (`APP_NEXUS_ONAIR`), because a
    /// restart re-reads every row into a session opened under whatever call Settings holds then,
    /// and a contact must not change its call with it. Nothing else reads it: the Cabrillo QSO
    /// line writes [`station_call`](Self::station_call), else [`FieldDayLog::mycall`], as it
    /// always has.
    pub on_air_call: String,
    /// ⭐ **The radio this contact was made on** (ADIF `MY_RIG`) — [`FieldDayLog::rig`] at the
    /// moment it was logged, the name the general log's own funnel stamps (`Engine::log_rig_now`
    /// in tempo-app), so a radio changed mid-event names the right one on each contact. `""`
    /// when none was known: no rig set, or a row logged before this was recorded. Journaled
    /// (`MY_RIG`) and carried into the Logbook by the merge; no contest file writes it.
    pub rig: String,
    /// **The power this contact was made at** (ADIF `TX_PWR`), as the general log writes it —
    /// watts, `"100"`, `"0.5"` — from [`FieldDayLog::tx_power`] at log time; `""` when none was
    /// set. Text, not a number, because this row is compared for dupes and derives `Eq`.
    /// Journaled and merged as [`rig`](Self::rig) is.
    pub tx_power: String,
    /// **The grid this contact was made from** (ADIF `MY_GRIDSQUARE`) — [`FieldDayLog::grid`] at
    /// log time, Settings ▸ Station's grid; `""` when none was set, or a row logged before this
    /// was recorded. Journaled under a private tag (`APP_NEXUS_MYGRID`: a VHF contest's own sent
    /// `GRID` already writes `MY_GRIDSQUARE`, and that one wins) and carried into the Logbook by
    /// the merge, marked as Nexus's own so the LoTW upload leaves it out.
    pub grid: String,
}

impl LoggedQso {
    /// The ADIF mode name behind this row's scoring class — the actual on-air submode
    /// when one was recorded, and the historical class map for a legacy row that has
    /// none.
    ///
    /// One function because **two emitters must not disagree about what mode a contact
    /// was worked on**: the contest ADIF journal and the general-log merge both answer
    /// this question, and a row exported as RTTY in one and FT8 in the other is a
    /// contact the operator cannot reconcile afterwards.
    ///
    /// ⚠️ **The class map below is the NO-EVIDENCE answer, not the phone answer.** `PH`
    /// is a scoring class covering SSB, AM and FM; a row that recorded which of them was
    /// on the air says so in [`submode`](Self::submode) and that wins here. `SSB` is what
    /// a row with nothing recorded still claims — the legacy rows, and a contact worked
    /// with no rig read-back to say which sideband — and inventing one from the band
    /// would be the claim nobody can check afterwards.
    pub fn recorded_mode(&self) -> &str {
        if self.submode.is_empty() {
            match self.mode.as_str() {
                "CW" => "CW",
                "PH" => "SSB",
                _ => "FT8",
            }
        } else {
            self.submode.as_str()
        }
    }

    /// ⭐ **Worked in a mode `rs` bans** ([`FdRuleset::banned_modes`](crate::fd_rules::FdRuleset::banned_modes):
    /// FT8 and FT4 at the Illinois QSO Party, the WSJT modes at Winter Field Day). Such a contact
    /// is logged and written to the files like any other, and counts for nothing
    /// ([`FieldDayLog::counts`]).
    ///
    /// ⚠️ **The RECORDED mode, never [`recorded_mode`](Self::recorded_mode).** A row that recorded
    /// none exports as `FT8` for want of anything better, and a guess is not a contact in a banned
    /// mode: an empty mode is never banned, as `FdRuleset::mode_banned` says.
    ///
    /// The same answer as `FdRuleset::mode_banned(&self.submode)` without its allocation, which the
    /// scorer's walk of every row on every snapshot must not make: the row's mode is stored trimmed
    /// and uppercase, and the loader takes a banned mode only uppercase and non-empty.
    pub fn banned_under(&self, rs: &crate::fd_rules::FdRuleset) -> bool {
        rs.banned_modes.iter().any(|&m| m == self.submode)
    }

    /// ⭐ **What the dupe key needs to know about this row's bird** — [`SatKey`]'s
    /// default (a terrestrial contact) for every row that has no [`sat`](Self::sat).
    ///
    /// One accessor rather than two field reads at each key site, because the two halves
    /// must always come from the SAME leg: a bird with no FM flag, or an FM flag with no
    /// bird, is a key nobody meant.
    pub fn sat_key(&self) -> crate::contest::SatKey<'_> {
        match &self.sat {
            Some(s) => crate::contest::SatKey {
                bird: &s.bird,
                single_channel_fm: s.single_channel_fm,
            },
            None => crate::contest::SatKey::default(),
        }
    }

    /// One slot of what THEY sent me (`""` when the row does not carry it).
    pub fn rcvd(&self, key: &str) -> &str {
        field_raw(&self.rx, key)
    }

    /// One slot of what I sent THEM (`""` when the row does not carry it).
    ///
    /// A single slot, from the ROW — never an exchange rendered from a session. An
    /// emitter that wants the whole sent exchange calls
    /// [`contest::sent_exchange`](crate::contest::sent_exchange), which also takes a row.
    pub fn sent(&self, key: &str) -> &str {
        field_raw(&self.tx, key)
    }

    /// The class they sent — Field Day's name for [`rcvd("CLASS")`](Self::rcvd).
    ///
    /// A convenience for the Field Day consumers that have not been generalised yet
    /// (the club sync, the scoreboard, the interop emitters). It reads the ROW, which
    /// is the direction that matters; the batch that generalises those surfaces reads
    /// `rx` by slot id instead and this goes away with the last caller.
    pub fn class(&self) -> &str {
        self.rcvd("CLASS")
    }

    /// The section they sent — Field Day's name for [`rcvd("SECTION")`](Self::rcvd).
    pub fn section(&self) -> &str {
        self.rcvd("SECTION")
    }
}

fn field_raw<'a>(vals: &'a [crate::contest::FieldValue], key: &str) -> &'a str {
    vals.iter()
        .find(|v| v.key == key)
        .map(|v| v.raw.as_str())
        .unwrap_or("")
}

/// Whether `q` counts under `rs` ([`FieldDayLog::counts`]): not a duplicate, not a satellite
/// contact the ruleset gives no credit (Winter Field Day), and not worked in a mode it bans.
/// Over a ruleset already in hand, because the scorer asks it of every row on every snapshot.
fn counts_under(rs: &crate::fd_rules::FdRuleset, q: &LoggedQso) -> bool {
    !q.dupe && (rs.satellite_credit || q.sat.is_none()) && !q.banned_under(rs)
}

/// ⭐ **One operator's share of a contest log** ([`FieldDayLog::operator_stats`]): what the
/// counting rows stamped with that operator add up to, under the contest's own scoring.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperatorStats {
    /// The [`LoggedQso::operator`] stamp, uppercase. `""` groups the rows that name nobody.
    pub operator: String,
    /// Contacts that count ([`FieldDayLog::qso_count`]'s rule: no dupe, no banned mode, no
    /// uncredited satellite).
    pub qsos: u32,
    /// Their QSO points, before multipliers, power tier and bonuses: the per-row points the
    /// log's score sums, so every operator's points add up to the log's.
    pub points: u32,
    /// The multipliers this operator worked FIRST ([`Scoring::new_mults_by_row`]).
    ///
    /// [`Scoring::new_mults_by_row`]: crate::contest::Scoring::new_mults_by_row
    pub new_mults: u32,
    /// Contacts in the last 10 and the last 60 minutes before `now`.
    pub last_10_min: u32,
    pub last_60_min: u32,
    /// The UTC clock hour with the most contacts — its start (Unix seconds) and its count —
    /// the earliest such hour on a tie. `(0, 0)` with no contacts.
    pub best_hour: (u64, u32),
    /// Contacts by band and mode class, `(band, mode, contacts)`, sorted by band then mode.
    pub band_mode: Vec<(String, String, u32)>,
}

/// ⭐ **What a station not yet worked would add to the score's multipliers** — the
/// question a contest band map asks of every spot ([`FieldDayLog::spot_mults`]).
///
/// Owned, so it is built once under whatever lock guards the log and then asked about each
/// spot with that lock released. The answer is the scorer's own
/// ([`Scoring::new_mults_of`](crate::contest::Scoring::new_mults_of)): the candidate row is
/// built the way a logged one is, with the entity and prefix resolved from the call by the
/// same resolvers, and the role this session logs under.
///
/// ⚠️ **A received value is known only from this station's own earlier contact.** A spot
/// carries no exchange, so a multiplier read from one (a county, a section, a zone, a grid)
/// is judged from the newest contact with this call in the log, and not at all for a station
/// never worked. Silence over a guess: a value from anywhere else is not a CONFIRMED multiplier.
/// What the operator's call-history file says such a station sends is asked separately, and
/// answered as a PREDICTION ([`predicted_mults`](Self::predicted_mults)).
#[derive(Debug, Clone)]
pub struct SpotMults {
    ruleset: &'static crate::fd_rules::FdRuleset,
    worked: crate::contest::MultsWorked,
    role: String,
    /// The slots this session's role receives: the only ones a prediction may fill.
    receives: Vec<&'static crate::contest::FieldSpec>,
    /// The received exchange of each call's newest contact, by uppercase call.
    rx_by_call: std::collections::HashMap<String, Vec<crate::contest::FieldValue>>,
}

impl SpotMults {
    /// `(rule id, value)` for each multiplier working `call` on `band` in `mode_class`
    /// (`"CW"` | `"PH"` | `"DIG"`) would add, empty for none. `submode` is the on-air mode
    /// when a spot names one: a contact in a mode the contest bans counts nothing.
    pub fn new_mults(
        &self,
        call: &str,
        band: &str,
        mode_class: &str,
        submode: Option<&str>,
    ) -> Vec<(&'static str, String)> {
        if submode.is_some_and(|m| self.ruleset.mode_banned(m)) {
            return Vec::new();
        }
        let call = call.trim().to_ascii_uppercase();
        let placed = crate::contest::resolve_call(&call);
        let prefix = crate::contest::wpx_prefix(&call);
        let row = crate::contest::ScoreRow {
            mode_class,
            band,
            role: &self.role,
            rx: self.rx_by_call.get(&call).map_or(&[], Vec::as_slice),
            entity: placed.map(|p| p.entity),
            prefix: prefix.as_deref(),
            relation: None,
        };
        self.ruleset.scoring.new_mults_of(&self.worked, &row)
    }

    /// The contest this log runs, as its rules-file event id (`ilqp`): what a call-history file
    /// is bound to.
    pub fn event(&self) -> &'static str {
        self.ruleset.event
    }

    /// The slots this session's role receives, in its receive order.
    pub fn receives(&self) -> &[&'static crate::contest::FieldSpec] {
        &self.receives
    }

    /// ⭐ **The multipliers a station never worked here is PREDICTED to add**, `(rule id,
    /// value)`, from what another source says it sends: `exchanges` is one received exchange per
    /// possibility (a county-line station gives one per county), each counted by the scorer's
    /// own keys exactly as [`new_mults`](Self::new_mults) counts a logged one. A multiplier any of
    /// them would add is listed once.
    ///
    /// Empty for a station this log has worked: its own earlier exchange is the answer, and
    /// [`new_mults`](Self::new_mults) gives it. A multiplier the call alone already gives (an
    /// entity, a prefix) is [`new_mults`](Self::new_mults)'s too, so it is never listed here.
    pub fn predicted_mults(
        &self,
        call: &str,
        band: &str,
        mode_class: &str,
        submode: Option<&str>,
        exchanges: &[Vec<crate::contest::FieldValue>],
    ) -> Vec<(&'static str, String)> {
        let call = call.trim().to_ascii_uppercase();
        if self.rx_by_call.contains_key(&call)
            || submode.is_some_and(|m| self.ruleset.mode_banned(m))
        {
            return Vec::new();
        }
        let confirmed = self.new_mults(&call, band, mode_class, submode);
        let placed = crate::contest::resolve_call(&call);
        let prefix = crate::contest::wpx_prefix(&call);
        let mut out: Vec<(&'static str, String)> = Vec::new();
        for rx in exchanges {
            let row = crate::contest::ScoreRow {
                mode_class,
                band,
                role: &self.role,
                rx,
                entity: placed.map(|p| p.entity),
                prefix: prefix.as_deref(),
                relation: None,
            };
            for m in self.ruleset.scoring.new_mults_of(&self.worked, &row) {
                if !confirmed.contains(&m) && !out.contains(&m) {
                    out.push(m);
                }
            }
        }
        out
    }
}

/// What [`FieldDayLog::merge_adif`] found in a journal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdifMerge {
    /// Rows it restored into the log.
    pub restored: usize,
    /// Records it could not read: no CALL, or cut off before their `<EOR>`. Rows it passed over
    /// on purpose (from a previous event, or already in the log) are neither.
    pub unreadable: usize,
}

/// ⭐ **A contact the operator took out of the live log — kept, never deleted.**
///
/// The rows are exactly as they stood in the log, so the entry IS the before-state and
/// [`FieldDayLog::restore`] puts it back unchanged: same time, serial and seq. One entry per
/// removal, so a county line (one contact logged as a row per county) goes and comes back as
/// one contact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedQso {
    /// The contact's rows, in log order — one for an ordinary contact, one per county for a
    /// county line.
    pub rows: Vec<LoggedQso>,
    /// When the operator removed it, Unix seconds.
    pub removed_unix: u64,
}

impl RemovedQso {
    /// What the contest screen restores this entry by: its first row's seq, which no other
    /// entry and no live row holds.
    pub fn id(&self) -> u64 {
        self.rows.first().map_or(0, |q| q.seq)
    }
}

/// Why a removal or a restore changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveRefusal {
    /// The log holds no contact to remove.
    Empty,
    /// The newest contact is not the one the caller named: one was logged, removed or
    /// restored in between.
    Changed,
    /// No removed contact has that id.
    NotRemoved,
    /// The same station was worked again since, on a band and mode the ruleset's dupe key
    /// counts as the same contact — putting this one back would duplicate it.
    WorkedAgain,
}

/// What a quick edit puts on a contact already in the log ([`FieldDayLog::edit_contact`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContactEdit {
    /// The call, corrected.
    pub call: String,
    /// What they sent: one `(slot id, raw)` vector per row of the contact, in row order — what
    /// [`FieldDayLog::log_fields_at`] takes for one row, so a county line gives one per county.
    pub rows: Vec<Vec<(String, String)>>,
    /// The band, as a row names it (`20m`).
    pub band: String,
    /// The mode on the air, as [`LoggedQso::recorded_mode`] names it (`CW`, `USB`, `FT8`); `""`
    /// keeps the row's.
    pub mode: String,
}

/// Why an edit changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditRefusal {
    /// No live contact has that call and time: it was removed or edited since it was shown.
    Changed,
    /// The edit names no call, or a band no contest file can carry.
    Incomplete,
    /// The edit gives another number of rows than the contact has: a county line keeps its count.
    Rows,
    /// The contact was worked through a satellite, whose pass gave it its band.
    Satellite,
}

/// A journal record's QSO_DATE (yyyymmdd) + TIME_ON (hhmmss) as Unix seconds. An
/// unparseable one stamps 0 and falls to the age gate (never a panic on garbage input).
fn record_when(f: &std::collections::HashMap<String, String>) -> u64 {
    let parse_dt = |d: &str, t: &str| -> Option<u64> {
        Some(unix_from_ymdhms(
            d.get(0..4)?.parse().ok()?,
            d.get(4..6)?.parse().ok()?,
            d.get(6..8)?.parse().ok()?,
            t.get(0..2)?.parse().ok()?,
            t.get(2..4)?.parse().ok()?,
            t.get(4..6)?.parse().ok()?,
        ))
    };
    match (f.get("QSO_DATE"), f.get("TIME_ON")) {
        (Some(d), Some(t)) => parse_dt(d, t).unwrap_or(0),
        _ => 0,
    }
}

/// ADIF `MODE` and `SUBMODE` → (mode class, actual mode): how the journal reads a row's mode back
/// ([`FieldDayLog::restore_row`]) — the reverse of the map in [`adif`](FieldDayLog::adif), so keep
/// the two in step — and how an edit reads the mode the operator picked ([`mode_of_recorded`]),
/// so an edited row is the row its next restore reads. A digital MODE keeps its identity as the
/// submode so RTTY/SSTV rows survive the round-trip.
fn class_of_adif(mode: Option<&str>, submode: &str) -> (&'static str, String) {
    // ⭐ **The PHONE family is three MODEs, not one, and that is why `SSB` cannot be
    // the whole of it.** `adif` writes a phone row's recorded mode through
    // `logbook::adif_submode`, which pairs `USB`/`LSB` as `<MODE:3>SSB` +
    // `<SUBMODE>` and lets `FM` and `AM` ride as MODEs of their own. Reading `MODE`
    // alone therefore dropped the sideband on every restart — and filed an FM
    // contact as a `DIG` row, which is the wrong mode class, the wrong dupe bucket
    // and TWO points instead of the one a phone QSO is worth.
    //
    // Only the two REGISTERED sideband spellings are adopted from `SUBMODE`: the
    // value goes back out through that same cascade on the next export, so an
    // unrecognised one would re-emit as a bare invalid `<MODE>` and be dropped.
    let raw_submode = submode.trim().to_ascii_uppercase();
    let sideband = if raw_submode == "USB" || raw_submode == "LSB" {
        raw_submode.clone()
    } else {
        String::new()
    };
    // ⛔ **AND THE DIGITAL SUBMODES, which this dropped** (operator review of 1.15.0,
    // findings A3 and L2 — reached independently by two lenses).
    //
    // `adif` writes a digital row through `logbook::adif_submode`, so a TempoFast
    // contact journals as `<MODE:4>MFSK <SUBMODE:9>TEMPOFAST`. Reading `MODE` alone
    // brought it back as bare "MFSK" after a mid-event restart — in the SUBMITTED Field
    // Day file and in the merged lifetime record, so an operator filtering "worked on
    // TempoFast", or a submode-sensitive award, never saw it. TEMPOFAST, TEMPODEEP, FT4,
    // FT2, Q65, FST4 and FST4W were all lost this way.
    //
    // The narrow filter above was justified by "an unrecognised one would re-emit as a
    // bare invalid MODE" — TRUE for a value ADIF has never heard of, and FALSE for
    // exactly these, because `promoted_submode` IS the registry of values that re-emit
    // correctly. It is the same function the general log uses for the same job, so the
    // two readers cannot disagree about what a row was worked on.
    //
    // ⚠️ The round-trip test covers RTTY, which survives because RTTY is a real ADIF
    // MODE and so never needed promoting — which is why the suite could not see this.
    match mode.map(|m| m.to_ascii_uppercase()) {
        Some(m) if m == "CW" => ("CW", String::new()),
        Some(m) if m == "SSB" => ("PH", sideband),
        Some(m) if m == "FM" || m == "AM" => ("PH", m),
        Some(m) => (
            "DIG",
            crate::logbook::promoted_submode(&raw_submode)
                .map_or(m, |promoted| promoted.to_ascii_uppercase()),
        ),
        None => ("DIG", String::new()),
    }
}

/// The mode class and actual mode a row records for `mode`, the on-air mode as
/// [`LoggedQso::recorded_mode`] names it (`CW`, `USB`, `FT4`): written as the ADIF the row would
/// be ([`adif_submode`](crate::logbook::adif_submode)) and read back as the journal reads it.
fn mode_of_recorded(mode: &str) -> (&'static str, String) {
    match crate::logbook::adif_submode(mode) {
        Some((parent, sub)) => class_of_adif(Some(parent), sub),
        None => class_of_adif(Some(mode), ""),
    }
}

/// `call` on `row` — and, when the call itself changed, the entity, continent and prefix resolved
/// again from it.
fn set_row_call(row: &mut LoggedQso, call: &str) {
    let changed_call = !row.call.eq_ignore_ascii_case(call);
    row.call = call.to_string();
    if changed_call {
        // ⚠️ **SCORING-GRADE, and the one place the "resolved once at log time"
        // rule on those three fields does NOT apply.** That rule is about
        // re-resolving a SHIPPED row against a country file that has since moved,
        // which would rescore contacts already submitted. This is a different
        // event: the callsign itself was wrong, so the entity, continent and prefix
        // derived from it describe a station that was never worked. Left alone they
        // would claim the busted call's country multiplier and its WPX prefix.
        // `None` is a real answer here — a call the country file cannot place
        // resolves to nothing, and must not keep the old station's.
        let placed = crate::contest::resolve_call(call);
        row.entity = placed.map(|p| p.entity.to_string());
        row.continent = placed.map(|p| p.continent.to_string());
        row.prefix = crate::contest::wpx_prefix(call);
    }
}

/// What became of one journal record ([`FieldDayLog::merge_adif`]).
enum RowFate {
    Restored,
    /// No CALL: not a record this log can hold.
    NoCall,
    /// Passed over on purpose: from a previous event, or already in the log.
    Passed,
    /// Older than the restore's window, and never taken back: no session's log holds it, and
    /// the journal restore keeps it ([`FieldDayLog::aged`]).
    Aged,
}

/// Why a journal record was kept out of the log it was restored into
/// ([`FieldDayLog::restore_journal`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// Its `CONTEST_ID` is another contest's.
    OtherContest,
    /// It was logged in another running of this contest — a rehearsal before it, for one
    /// ([`FdRuleset::running_of`](crate::fd_rules::FdRuleset::running_of)).
    OtherRunning,
}

/// A journal record the log did not load, kept as the journal held it so that every
/// rewrite of the journal carries it on ([`FieldDayLog::journal_adif`]).
#[derive(Clone, Debug, PartialEq, Eq)]
struct HeldRecord {
    why: Held,
    /// Its tags through its `<EOR>`, byte for byte.
    adif: String,
}

/// A journal record older than the restore's window — no session's log holds its contact — kept
/// as the journal held it until a copy of it is in the Logbook ([`FieldDayLog::aged`]).
#[derive(Clone, Debug, PartialEq, Eq)]
struct AgedRecord {
    /// Its tags through its `<EOR>`, byte for byte.
    adif: String,
    /// Its `CONTEST_ID`, trimmed: `""` names this log's contest, as it does to the restore.
    contest_id: String,
    when_unix: u64,
    /// Its `APP_NEXUS_QSEQ`, `0` when it carries none.
    seq: u64,
}

/// A contact the journal keeps past its window ([`FieldDayLog::aged`]), as the Logbook is asked
/// about it.
#[derive(Clone, Debug)]
pub(crate) struct AgedContact {
    /// The rules of its contest (its rules-file event id, its runnings); `None` for one the
    /// loaded rules do not hold.
    pub(crate) rules: Option<&'static crate::fd_rules::FdRuleset>,
    pub(crate) when_unix: u64,
    /// Its merge identity at the position asked about: `None` when it can have none — it
    /// carries no number, or names a contest the loaded rules do not hold.
    pub(crate) identity: Option<crate::contest::MergeIdentity>,
}

/// The highest [`LoggedQso::seq`] among the rows a journal restore let go, and when the latest
/// of them was logged ([`FieldDayLog::spent`]).
///
/// It rides the journal's header as `<APP_NEXUS_QSEQ_SPENT:n>seq:unix`, where every reader of
/// the records, an earlier build's included, passes it over — and only once a row has been let
/// go, so a journal that never let one go is written exactly as before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Spent {
    seq: u64,
    when_unix: u64,
}

impl Spent {
    const TAG: &'static str = "APP_NEXUS_QSEQ_SPENT";

    /// Each half as high as either's: the two need not come from one row, and a number
    /// higher than needed costs nothing.
    fn max(self, other: Spent) -> Spent {
        Spent {
            seq: self.seq.max(other.seq),
            when_unix: self.when_unix.max(other.when_unix),
        }
    }

    /// The header field, or nothing when no row was let go.
    fn header(&self) -> String {
        header_field(Self::TAG, self.seq, self.when_unix)
    }

    /// The field read back from a journal's header. One this build cannot read is nothing
    /// let go, never an error: the rows still number themselves.
    fn from_header(header: &str) -> Spent {
        header_value(header, Self::TAG)
            .map(|(seq, when_unix)| Spent { seq, when_unix })
            .unwrap_or_default()
    }
}

/// ⭐ **The latest Clear test contacts** ([`FieldDayLog::remove_all`]): the highest
/// [`LoggedQso::seq`] given by then, and when it was made ([`FieldDayLog::cleared`]). `seq` 0:
/// never.
///
/// It rides the journal's header as `<APP_NEXUS_CLEARED:n>seq:unix`, as [`Spent`] does, and only
/// once a clear was made, so a journal never cleared is written exactly as before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Cleared {
    seq: u64,
    when_unix: u64,
}

impl Cleared {
    const TAG: &'static str = "APP_NEXUS_CLEARED";

    /// ⭐ **Was the row with `seq`, logged at `when_unix`, logged before this clear — before it by
    /// its seq AND by its time?** Such a row numbers nothing in the serial run.
    ///
    /// Either half alone would be wrong somewhere, and in the direction that sends a number twice.
    /// The seq orders one session's rows whatever the clock does, where a clock stepped back would
    /// put a contact logged after the clear before it. The time keeps the clear off a later
    /// session's rows: a running's seqs may start again from 1 ([`FieldDayLog::spent`] counts
    /// within its own running), below a clear made a year before. A row is passed over only when
    /// both say so, so a half that misjudges a row can only count it: the run then skips a number,
    /// and never sends one twice.
    fn passed_over(&self, seq: u64, when_unix: u64) -> bool {
        seq <= self.seq && when_unix <= self.when_unix
    }

    /// The header field, or nothing when no clear was made.
    fn header(&self) -> String {
        header_field(Self::TAG, self.seq, self.when_unix)
    }

    /// The field read back from a journal's header: `None` when it holds none this build can read.
    fn from_header(header: &str) -> Option<Cleared> {
        header_value(header, Self::TAG).map(|(seq, when_unix)| Cleared { seq, when_unix })
    }
}

/// A journal header field holding a seq and a moment, `<TAG:n>seq:unix`: nothing for seq 0.
fn header_field(tag: &str, seq: u64, when_unix: u64) -> String {
    if seq == 0 {
        return String::new();
    }
    let value = format!("{seq}:{when_unix}");
    format!("<{tag}:{}>{value}\n", value.len())
}

/// A [`header_field`] read back out of a journal's header: `None` when it holds none that reads.
fn header_value(header: &str, tag: &str) -> Option<(u64, u64)> {
    let open = format!("<{tag}:");
    let at = header.to_ascii_uppercase().find(&open)?;
    let (len, tail) = header[at + open.len()..].split_once('>')?;
    let (seq, when) = tail
        .get(..len.trim().parse::<usize>().ok()?)?
        .split_once(':')?;
    Some((seq.trim().parse().ok()?, when.trim().parse().ok()?))
}

/// A dupe-checked contest log with scoring — and **the log IS the session**: its
/// lifetime is the session's, so its rows need no per-row session id.
#[derive(Debug)]
pub struct FieldDayLog {
    pub mycall: String,
    /// The run of the contest this log belongs to.
    ///
    /// ⭐ **Replaces `myexch: Exchange`, which was one sent exchange for the whole
    /// log.** The session holds the exchange being sent RIGHT NOW; what a given contact
    /// sent is on that contact's row. `cabrillo()` reads the row, and nothing
    /// downstream reads this to describe a past contact.
    pub session: crate::contest::ContestSession,
    pub band: String,
    /// The dial right now, in kHz (`0` = unknown) — kept beside [`band`](Self::band) by
    /// the engine and stamped onto each row as it is logged, exactly as the band is.
    pub dial_khz: u32,
    /// The on-air TX frequency right now (MHz, `0.0` = unknown) and the receive leg when
    /// split is genuinely in use — kept beside [`dial_khz`](Self::dial_khz) by the engine
    /// and stamped onto each row as it is logged, exactly as the band and the dial are.
    /// Both come from `Engine::log_frequencies`; see [`LoggedQso::on_air_mhz`] for why this
    /// is a different fact from the dial rather than a second copy of it.
    pub on_air_hz: u64,
    pub on_air_rx_hz: Option<u64>,
    pub event: FdEvent,
    /// The ACTUAL on-air digital mode currently keyed (ADIF-style name, e.g.
    /// "FT8", "FT4"), stamped by the engine at FD entry and on every tier
    /// change — the funnel that fills [`LoggedQso::submode`] for the digital
    /// sequencer's own [`log`](Self::log) calls, so a WFD RTTY/FT4 contact is
    /// never exported or pushed as "FT8". Applies to the "DIG" class only:
    /// CW/PH manual entries ARE their on-air mode and keep an empty submode.
    pub current_submode: String,
    /// The operator at the key RIGHT NOW (ADIF `OPERATOR`, uppercase; `""` = nobody named),
    /// stamped by the engine at FD entry, whenever the operator setting changes and before
    /// every manual contact, and copied onto each row as it is logged, exactly as
    /// [`current_submode`](Self::current_submode) is — so every log path, the digital
    /// sequencer's own included, names whoever was at the key for that contact.
    pub operator: String,
    /// The call being sent RIGHT NOW when it is not [`mycall`](Self::mycall), copied onto each
    /// row as it is logged exactly as [`operator`](Self::operator) is
    /// ([`LoggedQso::station_call`]). `""` on every log but a club's rebuild, which sets it
    /// for each row of a GOTA station's (`ClubLog::unique_log_with` in tempo-app).
    pub station_call: String,
    /// The station's call on the air RIGHT NOW, Settings' "Callsign on the air", stamped by the
    /// engine at contest entry and on every save, and copied onto each row as it is logged
    /// exactly as [`operator`](Self::operator) is ([`LoggedQso::on_air_call`]). `""` in a log
    /// nothing stamps.
    pub on_air_call: String,
    /// The radio RIGHT NOW (ADIF `MY_RIG`, `""` = none known), stamped by the engine at contest
    /// entry and whenever the station changes, and copied onto each row as it is logged exactly as
    /// [`operator`](Self::operator) is ([`LoggedQso::rig`]).
    pub rig: String,
    /// The station power RIGHT NOW (ADIF `TX_PWR` text, `""` = none set), stamped and copied as
    /// [`rig`](Self::rig) is ([`LoggedQso::tx_power`]).
    pub tx_power: String,
    /// The station grid RIGHT NOW (Settings ▸ Station, uppercase, `""` = none), stamped and
    /// copied as [`rig`](Self::rig) is ([`LoggedQso::grid`]).
    pub grid: String,
    /// The LIVE rows: what is scored, duped, exported, merged and synced.
    qsos: Vec<LoggedQso>,
    /// ⭐ **The contacts the operator took out of the live log — kept, never deleted**
    /// ([`remove_last`](Self::remove_last)), in the order they were removed.
    ///
    /// Outside [`qsos`](Self::qsos) rather than flagged inside it, so every reader of the live
    /// log is right without a filter: the score, the dupe index, the Cabrillo, the submitted
    /// ADIF, the general-log merge, the club outbox and the snapshot's `log` (whose length the
    /// hosted Remote page checks against the QSO count). The journal is the one writer that
    /// reads both, so a restart keeps a removed contact removed — and its numbers spent.
    removed: Vec<RemovedQso>,
    /// The dupe index, keyed by the ruleset's own [`DupeRule`](crate::contest::DupeRule)
    /// as an ordered `Vec<String>` rather than by a `(call, band, mode)` tuple — a
    /// mobile in a new county is a new station, and a tuple cannot say so.
    worked: HashSet<Vec<String>>,
    /// ⭐ **The `constant_sent` warning raised at LOG time** (§6.3) — `Some` once a
    /// contact has been logged whose sent value for a slot the role declares constant
    /// disagrees with the log's first row.
    ///
    /// It is stored rather than returned because the log path's answer is already
    /// "logged / dupe" and the guard **warns, never refuses**: the software cannot
    /// know whether the old rows or the new one carry the typo, so it must not throw
    /// away a real contact on a guess. The surface reads it and shows it; clearing it
    /// is the surface's business ([`clear_constant_sent_warning`](Self::clear_constant_sent_warning)).
    constant_sent_warning: Option<crate::contest::ConstantSentMismatch>,
    /// The next [`LoggedQso::seq`] to stamp. Starts at 1; a journal restore
    /// advances it past every restored seq so a fresh session's rows continue
    /// the per-position monotonic sequence instead of colliding with rows the
    /// club host already merged — and past the numbers of this running's rows the
    /// journal let go ([`spent`](Self::spent)).
    next_seq: u64,
    /// Journal records of another contest, or of another running of this one, that a
    /// restore kept out of this log — and keeps ([`restore_journal`](Self::restore_journal)).
    held: Vec<HeldRecord>,
    /// ⭐ **The numbers of the contacts the journal let go**: the highest
    /// [`LoggedQso::seq`] among the rows a restore passed over as older than its window, and
    /// when the latest of them was logged. Read from the journal's header and written back
    /// into it ([`journal_adif`](Self::journal_adif)), so it outlives the rows.
    ///
    /// An earlier build's restore let a running's rows go after four days, and their numbers
    /// went with them: a contact logged in the same running five days on was numbered 1 again,
    /// so its merge name was the first contact's and the merge took it for one already in the
    /// Logbook — as a club host holding `(position, 1)` would have. A session of the running
    /// they were logged in numbers on from them ([`restore_journal`](Self::restore_journal)).
    /// This build lets go no row of the running it restores: the header keeps those builds'
    /// numbers, and the numbers of the records it ages ([`aged`](Self::aged)).
    ///
    /// ⚠️ **A number kept beside the rows, where the serial run is re-derived from them**
    /// (`restore_row`), and the difference is which way each may err. A serial must follow
    /// the rows the Cabrillo file holds; this must only never repeat, and one higher than
    /// needed costs nothing. It holds the numbers of rows no longer in the file alone: every
    /// row still in it numbers itself.
    spent: Spent,
    /// ⭐ **The contacts the journal keeps past its window**: the records of another contest, or of
    /// another running of this one, that a journal restore found older than `min_when_unix` and
    /// never taken back ([`restore_journal`](Self::restore_journal)),
    /// kept as the journal held them and written back by every rewrite
    /// ([`journal_adif`](Self::journal_adif)). No session's log loads them.
    ///
    /// The restore let them go with the rest of the rows past its four days, and the next
    /// contact rewrote the journal without them, so a contact never merged into the Logbook was
    /// then in no file Nexus keeps. This log cannot see the Logbook, so it keeps every one; one
    /// goes only once a copy of it is on disk there, made at its own time
    /// ([`settle_aged`](crate::contest::settle_aged)), which is how a merged contact still ages
    /// out. Their numbers are [`spent`](Self::spent), as before. A record taken back
    /// (`APP_NEXUS_REMOVED`) is not kept: it ages out as it always did, and stays taken back.
    aged: Vec<AgedRecord>,
    /// ⭐ **Where the serial run started again: the latest Clear test contacts**
    /// ([`remove_all`](Self::remove_all)). Read from the journal's header and written back into it
    /// ([`journal_adif`](Self::journal_adif)), so it outlives a restart.
    ///
    /// A fact about an event, not a second counter: the run is still re-derived from the rows
    /// (`restore_row`), from the ones logged after the clear, as the live run counts them — a
    /// contact the clear took out numbers nothing, restored or not.
    cleared: Cleared,
}

impl FieldDayLog {
    /// A log for one run of one contest.
    ///
    /// [`event`](Self::event) is taken from the session rather than defaulted, so the
    /// two cannot disagree about which sponsor's rules are running — the class letter
    /// sets are disjoint, and a log whose event says ARRL while its exchange says
    /// Winter would validate `2O` against `ABCDEF`.
    pub fn new(mycall: &str, session: crate::contest::ContestSession, band: &str) -> Self {
        let event = FdEvent::from_contest_id(&session.contest_id);
        Self {
            mycall: mycall.to_string(),
            session,
            band: band.to_string(),
            dial_khz: 0,
            on_air_hz: 0,
            on_air_rx_hz: None,
            event,
            current_submode: String::new(),
            operator: String::new(),
            station_call: String::new(),
            on_air_call: String::new(),
            rig: String::new(),
            tx_power: String::new(),
            grid: String::new(),
            qsos: Vec::new(),
            removed: Vec::new(),
            worked: HashSet::new(),
            constant_sent_warning: None,
            next_seq: 1,
            held: Vec::new(),
            spent: Spent::default(),
            aged: Vec::new(),
            cleared: Cleared::default(),
        }
    }

    /// ⭐ **The ruleset governing this log — found through the SESSION's rules-file
    /// event id**, which is the only key that can name a contest that is not one of the
    /// two Field Day events.
    ///
    /// [`event`](Self::event) cannot: it is an [`FdEvent`], a two-arm enum, and
    /// `FdEvent::from_contest_id` maps everything that is not `WFD` onto ARRL Field
    /// Day. A Tennessee QSO Party log read through it would take ARRL Field Day's dupe
    /// rule (no exchange slots — so a mobile in a new county would read as a dupe), its
    /// points table and its Cabrillo id, and nothing anywhere would say so.
    ///
    /// The `unwrap_or_else` is not a fallback in the "and if not, guess" sense: the id
    /// came out of the rules table, which is a set-once `OnceLock`, so the lookup that
    /// found it cannot stop finding it. It keeps Field Day answering rather than
    /// panicking if that ever became false.
    pub fn ruleset(&self) -> &'static crate::fd_rules::FdRuleset {
        crate::fd_rules::ruleset_by_id(&self.session.event_id, self.session.rules_year)
            .unwrap_or_else(|| {
                crate::fd_rules::ruleset(self.event, crate::fd_rules::CURRENT_RULES_YEAR)
            })
    }

    /// The dupe key this log's ruleset declares, as data.
    ///
    /// Read from the ruleset each time rather than cached at construction, because
    /// [`event`](Self::event) is public and is assigned after `new` on several paths; a
    /// cached copy would silently answer for the wrong event.
    pub fn dupe_rule(&self) -> crate::contest::DupeRule {
        self.ruleset().dupe_rule
    }

    /// Point this log at a Field Day event, keeping [`event`](Self::event) and the
    /// SESSION in step.
    ///
    /// The two are one fact — `new` derives the first from the second — and assigning
    /// the public field alone is exactly the disagreement `new`'s own doc warns about:
    /// since the exports read the session, an `event` set by itself would flip the
    /// class letters and nothing else. Field Day only; a QSO party's session is built
    /// by [`ContestSession::for_ruleset`](crate::contest::ContestSession::for_ruleset)
    /// and never re-pointed.
    pub fn set_event(&mut self, event: FdEvent) {
        self.event = event;
        self.session.event_id = event.code().to_string();
        self.session.contest_id = event.contest_id().to_string();
    }

    /// The `constant_sent` warning raised at log time, if any (§6.3). `None` until a
    /// contact contradicts the log's first row.
    pub fn constant_sent_warning(&self) -> Option<&crate::contest::ConstantSentMismatch> {
        self.constant_sent_warning.as_ref()
    }

    /// Dismiss the log-time warning — the surface has shown it.
    pub fn clear_constant_sent_warning(&mut self) {
        self.constant_sent_warning = None;
    }

    /// ⭐ **FIRING SITE 2 of 3 (§6.3): the thorough scan, over the whole log**, with
    /// the row counts on each side. `None` = this log sends one value per constant slot
    /// and is one entry as far as that rule goes.
    ///
    /// Public because the club host runs it over its own merged rows (site 3) through
    /// the same function rather than a second walk that could disagree.
    pub fn constant_sent_scan(&self) -> Option<crate::contest::ConstantSentMismatch> {
        let spec = self.session.exchange;
        let first = self.qsos.first()?;
        crate::contest::constant_sent::scan(
            crate::contest::role_for(first, spec),
            self.qsos.iter().map(|q| q.tx.as_slice()),
        )
    }

    /// This log's dupe index, as the ruleset's own ordered keys.
    ///
    /// Exposed so the session-scoped B4 (§3.1) can UNION it with the general log's
    /// [`worked_keys_since`](crate::logbook::Logbook::worked_keys_since) instead of
    /// rebuilding it per snapshot — one index, one key shape, both halves built by the
    /// same [`DupeRule`](crate::contest::DupeRule).
    pub fn worked_keys(&self) -> &HashSet<Vec<String>> {
        &self.worked
    }

    /// The highest [`LoggedQso::seq`] this log has stamped or restored, or kept from a
    /// row the journal let go (`spent`) — the position's sync high-water mark
    /// (`join.max_seq`, and the base the outbox "rows past the host's ack" is computed
    /// from). 0 = empty log.
    pub fn max_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// Already worked this call on this band IN THIS MODE CLASS? (ARRL FD
    /// rules: each station counts once per band-mode — CW, digital and phone
    /// are separate contacts.) The digital sequencer always logs "DIG".
    pub fn is_dupe(&self, call: &str) -> bool {
        self.is_dupe_mode(call, "DIG")
    }

    /// The verdict BEFORE the exchange has been copied — call, band and mode class
    /// only. The sequencers ask this on hearing a bare CQ, when nothing else is known.
    ///
    /// ⚠️ **A rule that keys on exchange slots cannot be judged from these three, so it
    /// answers `false` rather than guessing.** That is the safe direction and it is not
    /// symmetric: under-reporting costs one duplicate contact that scores zero, while
    /// over-reporting REFUSES a legal one — which is the exact defect that makes the
    /// shipped engine unusable for a QSO party. Both Field Day events key on these
    /// three and nothing else, so for them this is the whole key, not a prefix of it.
    pub fn is_dupe_mode(&self, call: &str, mode: &str) -> bool {
        self.worked_key(call, &self.band, mode)
    }

    /// Whether an EXPLICIT `(call, band, mode class)` key is in the dupe
    /// index — unlike [`is_dupe_mode`](Self::is_dupe_mode) it does not assume
    /// the log's current band. The club sync uses it to subtract own-log keys
    /// from the club dupe set (only club-ONLY keys ship to the UI). It carries
    /// `is_dupe_mode`'s exchange-slot caveat for the same reason.
    pub fn worked_key(&self, call: &str, band: &str, mode: &str) -> bool {
        let rule = self.dupe_rule();
        if !rule.by_fields.is_empty() || !rule.by_sent_fields.is_empty() {
            return false;
        }
        // ⚠️ TERRESTRIAL, because that is all this signature can say. A caller with a
        // bird in hand has no way to name it here, so a satellite contact is judged
        // against the band it was worked on and reads as NOT worked. That is the
        // direction this module insists on — under-reporting costs a duplicate that
        // scores zero; over-reporting refuses a legal contact — and the exact verdict
        // still happens at log time, where the leg is in hand.
        self.worked.contains(&rule.key_of(
            call,
            band,
            mode,
            &[],
            &[],
            crate::contest::SatKey::default(),
        ))
    }

    /// The exact verdict, over a whole candidate contact — the check `log_submode_at`
    /// itself makes, exposed so a caller can ask before it commits.
    pub fn is_dupe_row(
        &self,
        call: &str,
        band: &str,
        mode: &str,
        rx: &[crate::contest::FieldValue],
        tx: &[crate::contest::FieldValue],
    ) -> bool {
        // Terrestrial, for the same reason and with the same consequence as
        // [`worked_key`](Self::worked_key): this signature cannot name a bird.
        self.worked.contains(&self.dupe_rule().key_of(
            call,
            band,
            mode,
            rx,
            tx,
            crate::contest::SatKey::default(),
        ))
    }

    /// ⭐ **THE PER-RULESET SPLIT, and the one place it is decided.** Take a candidate
    /// row's key into the dupe index and say how the row must be written: `Some(false)`
    /// a fresh contact, `Some(true)` a duplicate that is logged and marked, `None` the
    /// ruleset refuses it and nothing is written.
    ///
    /// A duplicate is either a row that scores zero or a contact that was never made,
    /// and which one is the SPONSOR's answer — it rides on the ruleset's own
    /// [`DupeRule::log_dupes`](crate::contest::DupeRule::log_dupes), where both
    /// sponsors' sentences are quoted. A contest that cross-checks wants the row
    /// (removing it hands the other operator a NIL). Since the operator's ruling of
    /// 2026-10-11 every shipped contest logs its dupes, Field Day included, so no shipped
    /// contest reaches `None`; the refusal stays for a sponsor whose rules say to remove
    /// dupes, which would be refused where rulesets are built (`fd_rules::build`).
    ///
    /// Both write paths — [`log_exchange_at`](Self::log_exchange_at) and the journal's
    /// `restore_row` — come through here, because two copies of this decision is how
    /// the live log and the restored one come to disagree about the same contact.
    fn admit_row(&mut self, key: Vec<String>, satellite: bool) -> Option<bool> {
        // ⭐ A satellite contact this ruleset gives no credit (Winter Field Day) is logged,
        // worth nothing, and a dupe of nothing: kept out of the index, it can never refuse a
        // later contact that does count.
        if satellite && !self.ruleset().satellite_credit {
            return Some(false);
        }
        self.admit(key)
    }

    fn admit(&mut self, key: Vec<String>) -> Option<bool> {
        let dupe = self.worked.contains(&key);
        if dupe && !self.dupe_rule().log_dupes {
            return None;
        }
        self.worked.insert(key);
        Some(dupe)
    }

    /// ⭐ **Rebuild the dupe index and re-derive every row's mark, in row order** — the
    /// BATCH form of the rule [`admit`](Self::admit) applies one row at a time.
    ///
    /// ⚠️ **A correction cannot patch the index, it has to rebuild it**, for two
    /// reasons that only became true together. The index is a set of keys with no way
    /// back to the rows holding them, and under a reporting ruleset SEVERAL rows can
    /// legitimately share one key — that is what a logged duplicate is — so removing
    /// the corrected row's old key would un-dupe rows that are still duplicates.
    /// And the mark is ORDER-derived, so correcting one row can make a LATER row stop
    /// being a duplicate, or start being one; neither is reachable by looking at the
    /// edited row alone.
    ///
    /// ⚠️ It must agree with [`admit`](Self::admit) exactly, being the same rule said
    /// twice — so a rebuild of a log built by logging changes nothing, and there is a
    /// test that says so. `log_dupes` does not appear here on purpose: refusing happens
    /// when a row is offered, and every row already in the log was already admitted.
    fn rebuild_dupe_index(&mut self) {
        let rule = self.dupe_rule();
        let credit = self.ruleset().satellite_credit;
        let mut worked: HashSet<Vec<String>> = HashSet::new();
        for q in &mut self.qsos {
            // A satellite contact the ruleset gives no credit stays out, as `admit_row`
            // keeps it out.
            if !credit && q.sat.is_some() {
                q.dupe = false;
                continue;
            }
            // `insert` is false when the key was already present — an earlier row has
            // it, which is precisely what makes this row a duplicate.
            q.dupe = !worked.insert(rule.key(q));
        }
        self.worked = worked;
    }

    /// ⭐ **Correct a contact already in the log** — a busted callsign or a wrong band,
    /// the two corrections [`Logbook::update_record`](crate::logbook::Logbook::update_record)
    /// names in its own doc. Returns false, having changed nothing, when `seq` names no
    /// row here.
    ///
    /// ⚠️ **Why this exists at all:** the ADIF and the Cabrillo are written from THESE
    /// rows. A correction that reached the general log and stopped there left the
    /// submitted Cabrillo carrying the busted call while the log the operator can read
    /// showed the corrected one — a divergence in the file that gets scored, with
    /// nothing on screen to show for it.
    ///
    /// **`seq` is the identity, because a contest row has no qid.** The general log
    /// names one of these rows by `APP_NEXUS_QID`, which is MINTED from the seq at
    /// merge time; [`contest::merged_row`](crate::contest::merged_row) turns it back,
    /// and refuses a qid belonging to another session or position. A zero seq is
    /// UNSTAMPED rather than "the first row" — the merge refuses those rows, so the
    /// general log holds nothing that could address one, and it is refused here too.
    ///
    /// **Out of scope, deliberately, rather than half-done:** the exchange (`rx`/`tx`)
    /// and the mode. The exchange is a typed field vector rather than the two strings
    /// below, so accepting one means resolving it through the spec, and editing the
    /// SENT side is also what would leave
    /// [`constant_sent_warning`](Self::constant_sent_warning) describing rows that no
    /// longer exist. The mode arrives from the general log as an ADIF name and would
    /// need the ADIF→(class, submode) mapping that the journal restore already owns —
    /// a second place for it is how two readings of one contact appear.
    pub fn correct_row(&mut self, seq: u64, call: &str, band: &str) -> bool {
        // A row is addressed by a STAMPED seq. 0 means the row never got one.
        if seq == 0 {
            return false;
        }
        let Some(row) = self.qsos.iter_mut().find(|q| q.seq == seq) else {
            return false;
        };
        set_row_call(row, call.trim());
        row.band = band.trim().to_string();
        // The call and the band are both dupe-key components, so the correction can
        // move the mark on rows it did not touch. Rebuild rather than patch.
        self.rebuild_dupe_index();
        true
    }

    /// ⭐ **Edit a contact already in the log — its call, what they sent, its band and its
    /// mode — and keep everything that makes it the contact it was**: its time, its seq, the
    /// operator and the exchange it was sent with, the serial included. An edit never spends a
    /// number, so the other station's copy of the serial still matches, and a journal restore
    /// rebuilds the same numbers from the rows.
    ///
    /// The contact is the live rows with `call` (any case) and `when_unix` — what the screen
    /// showed the operator, as for [`remove_last`](Self::remove_last) — one row, or one per
    /// county for a county line, each taking its own vector of [`ContactEdit::rows`] in row
    /// order. A row with no time (`0`, a legacy row) cannot be told from another of the same
    /// call, so it is never named. Returns the rows as they were.
    ///
    /// ⚠️ **The rows are rebuilt, not patched.** The received side is resolved through the
    /// running exchange as [`log_fields_at`](Self::log_fields_at) resolves it; a new call
    /// resolves its entity, continent and prefix again ([`correct_row`](Self::correct_row) says
    /// why that is right here); a new band drops the row's frequencies, which were read on the
    /// old one and would put a 20 m frequency on a 40 m contact (unknown is the honest answer,
    /// and both files then write the band); a new mode is read as the journal reads the ADIF the
    /// row is written as, and a new mode class re-fits the RST it was sent
    /// ([`ContestSession::fit_sent_rst`](crate::contest::ContestSession::fit_sent_rst)). The
    /// dupe index and every row's mark are rebuilt, because an edit can make a later row a
    /// duplicate, or stop it being one.
    pub fn edit_contact(
        &mut self,
        call: &str,
        when_unix: u64,
        edit: &ContactEdit,
    ) -> Result<Vec<LoggedQso>, EditRefusal> {
        let at: Vec<usize> = self
            .qsos
            .iter()
            .enumerate()
            .filter(|(_, q)| {
                when_unix != 0
                    && q.when_unix == when_unix
                    && q.call.eq_ignore_ascii_case(call.trim())
            })
            .map(|(i, _)| i)
            .collect();
        if at.is_empty() {
            return Err(EditRefusal::Changed);
        }
        let call = edit.call.trim().to_ascii_uppercase();
        let band = edit.band.trim().to_ascii_lowercase();
        if call.is_empty() || band_to_cabrillo_freq(&band).is_none() {
            return Err(EditRefusal::Incomplete);
        }
        if edit.rows.len() != at.len() {
            return Err(EditRefusal::Rows);
        }
        // A pass gave a satellite contact its band, its frequency and its bird together: a band
        // typed over it would leave a row worked through the bird on a band the bird is not on.
        if at
            .iter()
            .any(|&i| self.qsos[i].sat.is_some() && !self.qsos[i].band.eq_ignore_ascii_case(&band))
        {
            return Err(EditRefusal::Satellite);
        }
        let mode = edit.mode.trim().to_ascii_uppercase();
        let before: Vec<LoggedQso> = at.iter().map(|&i| self.qsos[i].clone()).collect();
        let spec = self.session.exchange;
        for (&i, fields) in at.iter().zip(&edit.rows) {
            let row = &mut self.qsos[i];
            set_row_call(row, &call);
            row.rx = fields
                .iter()
                .filter_map(|(k, v)| spec.copied(k, v))
                .collect();
            if !row.band.eq_ignore_ascii_case(&band) {
                row.band = band.clone();
                row.freq_khz = 0;
                row.on_air_hz = 0;
                row.freq_rx_hz = None;
            }
            if !mode.is_empty() && mode != row.recorded_mode().to_ascii_uppercase() {
                let (class, submode) = mode_of_recorded(&mode);
                if class != row.mode {
                    self.session.fit_sent_rst(&mut row.tx, class);
                }
                row.mode = class.to_string();
                row.submode = submode;
            }
        }
        self.rebuild_dupe_index();
        Ok(before)
    }

    /// The newest contact in the live log: the trailing rows sharing the newest row's call
    /// and time, which is one row for an ordinary contact and one per county for a county
    /// line. Empty when the log is.
    pub fn newest_contact(&self) -> &[LoggedQso] {
        let Some(last) = self.qsos.last() else {
            return &[];
        };
        let n = self
            .qsos
            .iter()
            .rev()
            .take_while(|q| q.call == last.call && q.when_unix == last.when_unix)
            .count();
        &self.qsos[self.qsos.len() - n..]
    }

    /// ⭐ **Take the newest contact out of the live log — removed, never deleted** — when it is
    /// still the contact the caller named: `call` (any case) and `when_unix`, which is what
    /// the entry strip showed the operator. A county line goes with all its rows.
    ///
    /// `Changed` when the newest contact is another one (logged, removed or restored in
    /// between), `Empty` when there is none; either way nothing changes. Only the NEWEST,
    /// because that is the contact an operator who just stopped a TU means, and the strip
    /// names it before anything happens.
    ///
    /// ⚠️ **The numbers stay spent.** [`next_seq`](Self::max_seq) and the session's serial run
    /// are untouched, so the next contact gets the next number: the other station may already
    /// hold this one's serial, and the club host treats a reissued seq as a repeat and drops
    /// the next real contact. The journal keeps the rows (marked removed), so a restart, which
    /// rebuilds both numbers from the rows, cannot reissue them either.
    pub fn remove_last(
        &mut self,
        call: &str,
        when_unix: u64,
        removed_unix: u64,
    ) -> Result<RemovedQso, RemoveRefusal> {
        let newest = self.newest_contact();
        let Some(first) = newest.first() else {
            return Err(RemoveRefusal::Empty);
        };
        if !first.call.eq_ignore_ascii_case(call.trim()) || first.when_unix != when_unix {
            return Err(RemoveRefusal::Changed);
        }
        let at = self.qsos.len() - newest.len();
        let rows = self.qsos.split_off(at);
        Ok(self.took_out(rows, removed_unix))
    }

    /// Take ONE row out of the live log by its seq — the contest half of a Logbook delete of
    /// that row's merged copy, which names one row, never a whole county line. `None`, having
    /// changed nothing, when no live row carries `seq` (`0` is unstamped and names none).
    pub fn remove_row(&mut self, seq: u64, removed_unix: u64) -> Option<RemovedQso> {
        if seq == 0 {
            return None;
        }
        let at = self.qsos.iter().position(|q| q.seq == seq)?;
        let row = self.qsos.remove(at);
        Some(self.took_out(vec![row], removed_unix))
    }

    /// ⭐ **Take every contact out of the live log — removed, never deleted** — the contest
    /// screen's Clear test contacts, when the log is still what the screen named: `row_count` live
    /// rows, the newest contact `call` (any case) at `when_unix`. Each contact is its own entry,
    /// oldest first, as [`Self::remove_last`] would make it (a county line one entry with all its
    /// rows), so each comes back on its own ([`Self::restore`]).
    ///
    /// `Changed` when the log is not what was named (a contact logged, removed or restored in
    /// between), `Empty` when it holds none; either way nothing changes. ⚠️ **The seqs stay
    /// spent, as for [`Self::remove_last`], and the serial run starts again at 1**: the next
    /// contact's seq follows the cleared ones, and its serial is 1, after a restart too
    /// (`cleared`). A contact restored after the clear keeps the serial it was sent and does not
    /// move the run, so a later contact may send its number again.
    pub fn remove_all(
        &mut self,
        row_count: usize,
        call: &str,
        when_unix: u64,
        removed_unix: u64,
    ) -> Result<Vec<RemovedQso>, RemoveRefusal> {
        let newest = self.newest_contact();
        let Some(first) = newest.first() else {
            return Err(RemoveRefusal::Empty);
        };
        if self.qsos.len() != row_count
            || !first.call.eq_ignore_ascii_case(call.trim())
            || first.when_unix != when_unix
        {
            return Err(RemoveRefusal::Changed);
        }
        // A contact is the run of rows sharing one call and one time, as `newest_contact` reads
        // the newest.
        let mut entries = Vec::new();
        let mut contact: Vec<LoggedQso> = Vec::new();
        for row in std::mem::take(&mut self.qsos) {
            if contact
                .last()
                .is_some_and(|q| q.call != row.call || q.when_unix != row.when_unix)
            {
                entries.push(self.took_out(std::mem::take(&mut contact), removed_unix));
            }
            contact.push(row);
        }
        entries.push(self.took_out(contact, removed_unix));
        // ⭐ **The serial run starts again at 1, and the seq does not**: test contacts cleared
        // before the start leave the contest to begin at 001, while a club host drops a reissued
        // seq as a repeat. Only here: a removal gives no number back, even of the last contact in
        // the log. Kept as the clear itself, so a restart counts the run from the rows the live
        // log counts it from.
        self.cleared = Cleared {
            seq: self.max_seq(),
            when_unix: removed_unix,
        };
        self.session.restart_serials();
        Ok(entries)
    }

    /// The one place rows leave the live log: kept as one entry, the dupe index and every
    /// row's mark rebuilt (removing a row can make a LATER one stop being a duplicate), and
    /// a showing `constant_sent` warning re-derived, since the row it described may be the
    /// one that went.
    fn took_out(&mut self, rows: Vec<LoggedQso>, removed_unix: u64) -> RemovedQso {
        let entry = RemovedQso { rows, removed_unix };
        self.removed.push(entry.clone());
        self.rebuild_dupe_index();
        if self.constant_sent_warning.is_some() {
            self.constant_sent_warning = self.constant_sent_scan();
        }
        entry
    }

    /// ⭐ **Put a removed contact back exactly as it was** — same time, serial and seq, each
    /// row in its seq place — by its [`id`](RemovedQso::id).
    ///
    /// `WorkedAgain`, having changed nothing, when the same station was worked again SINCE
    /// the removal — a live row holding one of its dupe keys, logged at or after the removal.
    /// Under a ruleset that refuses duplicates it is ANY live row holding one, because two
    /// live rows never share a key there (a contact restored in between is one). Under one
    /// that logs them, a duplicate of a contact worked BEFORE the removal was a dupe then and
    /// restores as one. `NotRemoved` when no entry has that id.
    pub fn restore(&mut self, id: u64) -> Result<RemovedQso, RemoveRefusal> {
        let Some(at) = self.removed.iter().position(|r| r.id() == id) else {
            return Err(RemoveRefusal::NotRemoved);
        };
        let rule = self.dupe_rule();
        let credit = self.ruleset().satellite_credit;
        // A satellite contact the ruleset gives no credit is a dupe of nothing, on either side.
        let keyed = |q: &&LoggedQso| credit || q.sat.is_none();
        let entry = &self.removed[at];
        let worked_again = entry.rows.iter().filter(keyed).any(|r| {
            let key = rule.key(r);
            self.qsos.iter().filter(keyed).any(|q| {
                (!rule.log_dupes || q.when_unix >= entry.removed_unix) && rule.key(q) == key
            })
        });
        if worked_again {
            return Err(RemoveRefusal::WorkedAgain);
        }
        let entry = self.removed.remove(at);
        for row in entry.rows.iter().cloned() {
            let place = self
                .qsos
                .iter()
                .position(|q| q.seq > row.seq)
                .unwrap_or(self.qsos.len());
            self.qsos.insert(place, row);
        }
        self.rebuild_dupe_index();
        if self.constant_sent_warning.is_none() {
            self.constant_sent_warning = self.constant_sent_scan();
        }
        Ok(entry)
    }

    /// The removed contacts, oldest removal first — kept by the log and its journal, outside
    /// every count and export.
    pub fn removed(&self) -> &[RemovedQso] {
        &self.removed
    }

    /// ⭐ **Every row's QSO number, by its [`seq`](LoggedQso::seq)** — live rows and removed ones,
    /// in every contest, the number the log table shows and the export writes as ADIF `STX`.
    ///
    /// In a contest that sends a serial it IS the serial the row was sent ([`issued_serial`]),
    /// because a second count beside the one that went on the air would show the operator a
    /// number nobody copied. A row that carries none has no number.
    ///
    /// In every other contest it is the row's place in its run, in the order the rows were
    /// logged (their seq: a contact typed in late with an earlier time takes the next number and
    /// moves nobody, as N1MM numbers it). **Removed rows are counted**, so a number once shown is
    /// never given to another contact, after a restart too (the journal keeps removed rows); and
    /// a run is the rows since the latest Clear test contacts (`cleared`), which starts the count
    /// again at 1 as it starts the serial run again. A row restored after the clear is counted in
    /// the run it was logged in.
    ///
    /// ⚠️ **Read off the rows, never stored beside them** — the reason `restore_row` gives for
    /// the serial run. A journal written before numbers existed loads exactly as it did and is
    /// numbered by the order of its rows (a restore gives a row without a seq the next one, in
    /// file order); nothing is rewritten.
    pub fn qso_numbers(&self) -> std::collections::HashMap<u64, u32> {
        let rows = self
            .qsos
            .iter()
            .chain(self.removed.iter().flat_map(|r| &r.rows))
            .filter(|q| q.seq != 0);
        if self.session.serial_now().is_some() {
            let spec = self.session.exchange;
            return rows
                .filter_map(|q| issued_serial(spec, &q.tx).map(|n| (q.seq, n)))
                .collect();
        }
        let mut order: Vec<(bool, u64)> = rows
            .map(|q| (self.cleared.passed_over(q.seq, q.when_unix), q.seq))
            .collect();
        order.sort_unstable();
        let mut out = std::collections::HashMap::with_capacity(order.len());
        let mut run = None;
        let mut n = 0;
        for (before_clear, seq) in order {
            if run != Some(before_clear) {
                run = Some(before_clear);
                n = 0;
            }
            n += 1;
            out.insert(seq, n);
        }
        out
    }

    /// Log a contact.
    ///
    /// ⭐ **The bool is "did this contact ENTER THE LOG", which is no longer the same
    /// question as "was it new".** Under a ruleset that reports its duplicates
    /// ([`DupeRule::log_dupes`](crate::contest::DupeRule::log_dupes)) a dupe is written,
    /// marked and returns `true` — and it must, because every caller uses this bool to
    /// decide whether to JOURNAL the log, and a row that is not journaled is lost on a
    /// restart. `false` means nothing was written, which no shipped contest does with a
    /// dupe any more: each logs its dupes. A caller that wants the other question has
    /// [`is_dupe_row`](Self::is_dupe_row) before the fact and
    /// [`LoggedQso::dupe`] after it.
    pub fn log(&mut self, call: &str, class: &str, section: &str, slot: u64) -> bool {
        self.log_mode_at(call, class, section, "DIG", slot, now_unix())
    }

    /// As [`log`](Self::log) with an explicit timestamp (tests / replays).
    pub fn log_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        self.log_mode_at(call, class, section, "DIG", slot, when_unix)
    }

    /// All-mode entry (the CW/Phone cockpits log FD contacts too): mode is the
    /// scoring class "DIG" | "CW" | "PH".
    pub fn log_mode_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        mode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        // The "DIG" class covers many on-air modes, so a digital entry stamps
        // [`current_submode`](Self::current_submode) (what the engine says is
        // actually keyed). CW IS its on-air mode and needs no submode.
        //
        // ⚠️ **"PH" is NOT filled here, and that is deliberate.** It covers SSB, AM and
        // FM, so it needs one too — but `current_submode` tracks the FT tier alone, and
        // reading it here would stamp "FT8" on a phone contact. The evidence for a phone
        // row (the rig's read-back, the station's commanded class) lives above this crate,
        // so the caller that HAS it passes it to [`log_submode_at`](Self::log_submode_at)
        // — `Engine::phone_on_air_mode`, through `fd_log_manual`. A caller with no such
        // evidence lands here and the row keeps the class alone, which
        // [`LoggedQso::recorded_mode`] exports as plain `SSB`, exactly as it always did.
        let submode = if mode.eq_ignore_ascii_case("DIG") {
            self.current_submode.clone()
        } else {
            String::new()
        };
        self.log_submode_at(call, class, section, mode, &submode, slot, when_unix)
    }

    /// As [`log_mode_at`](Self::log_mode_at) but also recording the ACTUAL
    /// on-air mode behind the scoring class (e.g. class "DIG", submode "RTTY")
    /// so exports emit the real mode. Dupes stay keyed on the CLASS — FD/WFD
    /// digital is ONE mode class, so an FT8 QSO dupes the same-band RTTY one.
    #[allow(clippy::too_many_arguments)] // log_mode_at + the one extra field
    pub fn log_submode_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        // Both sides, as data, BEFORE the dupe check — the key reads them (a mobile in
        // a new county is a new station, in both directions), so a check that ran first
        // would be judging a different contact from the one about to be logged.
        let rx = self.received(class, section);
        let tx = self.session.tx_for_row(call, mode);
        self.log_exchange_at(call, rx, tx, mode, submode, slot, when_unix)
    }

    /// [`log_submode_at`](Self::log_submode_at) with BOTH sides of the exchange given
    /// rather than derived — the club host's path.
    ///
    /// ⭐ The host merges rows from every position, and each of them sent its OWN
    /// exchange. Deriving the sent side here would stamp the host's on all of them,
    /// which is harmless for one Field Day club and wrong the moment two positions send
    /// different exchanges. So the caller that HAS the row's own sides passes them, and
    /// [`log_submode_at`](Self::log_submode_at) — the position's own logging path,
    /// where the session IS the source — is one call into this.
    #[allow(clippy::too_many_arguments)] // both exchange sides plus log_mode_at's own
    pub fn log_exchange_at(
        &mut self,
        call: &str,
        rx: Vec<crate::contest::FieldValue>,
        tx: Vec<crate::contest::FieldValue>,
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        self.log_exchange_on(call, rx, tx, mode, submode, slot, when_unix, None)
    }

    /// The one write site, with the optional satellite leg every other entry point
    /// passes `None` for. See [`SatLeg`] for why the exception exists and why it is the
    /// downlink.
    #[allow(clippy::too_many_arguments)] // log_exchange_at's own, plus the leg
    fn log_exchange_on(
        &mut self,
        call: &str,
        rx: Vec<crate::contest::FieldValue>,
        tx: Vec<crate::contest::FieldValue>,
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
        sat: Option<&SatLeg>,
    ) -> bool {
        let mode = mode.to_ascii_uppercase();
        // ⭐ THE ROW'S BAND, AND FOR A BIRD IT IS NOT THE LOG'S. Resolved before the dupe
        // key, which reads it: a station worked through the transponder and again on the
        // HF run is two contacts.
        let band = match sat.filter(|s| !s.band.trim().is_empty()) {
            Some(s) => s.band.clone(),
            None => self.band.clone(),
        };
        // ⭐ THE BIRD IS PART OF THE KEY (ARRL 7.3.8) — read from the leg the caller
        // named, so the contact the operator just made is judged by the same rule the
        // row will carry.
        let sat_key = match sat {
            Some(s) => crate::contest::SatKey {
                bird: &s.bird,
                single_channel_fm: s.single_channel_fm,
            },
            None => crate::contest::SatKey::default(),
        };
        let key = self
            .dupe_rule()
            .key_of(call, &band, &mode, &rx, &tx, sat_key);
        // The per-ruleset split lives in [`admit`](Self::admit) — one decision, shared
        // with the journal restore, so the live log and the restored one cannot disagree.
        let Some(dupe) = self.admit_row(key, sat.is_some()) else {
            return false;
        };
        // ⭐ FIRING SITE 1 of 3 (§6.3): the cheap one, at the moment the operator can
        // still fix it. O(1) — a rule that says "every row carries the same value" is
        // fully checked by comparing this row against the first. It WARNS: the contact
        // below is logged either way.
        if let Some(first) = self.qsos.first() {
            if let Some(m) = crate::contest::constant_sent::against_first(
                crate::contest::role_for(first, self.session.exchange),
                &first.tx,
                &tx,
            ) {
                self.constant_sent_warning = Some(m);
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        // ⭐ RESOLVED ONCE, HERE, and never again: what a row counts for is a fact about
        // the moment it was logged. A per-snapshot re-resolution would let a country-file
        // update silently rescore contacts already in a submitted log.
        let placed = crate::contest::resolve_call(call);
        self.qsos.push(LoggedQso {
            call: call.to_string(),
            rx,
            tx,
            role: self.session.role().id.to_string(),
            entity: placed.map(|p| p.entity.to_string()),
            continent: placed.map(|p| p.continent.to_string()),
            prefix: crate::contest::wpx_prefix(call),
            band,
            // The pass's downlink where there is one, the log's dial otherwise. A leg
            // with no frequency (`down_hz == 0`) still names the band and falls back
            // here for the numbers, exactly as an ordinary row does.
            freq_khz: match sat.filter(|s| s.down_hz > 0) {
                Some(s) => (s.down_hz as f64 / 1000.0).round() as u32,
                None => self.dial_khz,
            },
            on_air_hz: match sat.filter(|s| s.down_hz > 0) {
                Some(s) => s.down_hz,
                None => self.on_air_hz,
            },
            // ⚠️ NEVER ON A SATELLITE ROW. `FREQ_RX` equal to anything is a claim that
            // the QSO was worked SPLIT ON ONE BAND, and the uplink is not that — which
            // is the same call `Engine::log_frequencies` makes for the general log.
            freq_rx_hz: if sat.is_some() {
                None
            } else {
                self.on_air_rx_hz
            },
            mode,
            submode: submode.trim().to_ascii_uppercase(),
            slot,
            when_unix,
            seq,
            dupe,
            sat: sat.cloned(),
            operator: self.operator.clone(),
            station_call: self.station_call.clone(),
            on_air_call: self.on_air_call.clone(),
            rig: self.rig.clone(),
            tx_power: self.tx_power.clone(),
            grid: self.grid.clone(),
        });
        // The contact is logged, so nothing is in flight any more: the next exchange
        // composed issues its own serial instead of re-sending this one's.
        self.session.clear_in_flight();
        true
    }

    /// ⭐ **Log a contact whose received exchange is a FIELD VECTOR** — the path a
    /// contest that receives something other than `(class, section)` logs through.
    ///
    /// `fields` is `(slot id, raw)` in the role's receive order, exactly as the entry
    /// strip renders its boxes. Values are resolved through
    /// [`ExchangeSpec::copied`](crate::contest::ExchangeSpec::copied), so a
    /// [`FieldKind::OneOf`](crate::contest::FieldKind::OneOf) slot records the arm that
    /// actually matched — which is what the multiplier bucket and the ADIF column are
    /// later chosen by (§2.4). A pair naming a slot this exchange does not declare is
    /// DROPPED rather than invented: a value with no slot is not a value.
    ///
    /// [`log_submode_at`](Self::log_submode_at) is the Field Day shape of this call and
    /// stays, because the FT sequencer and the club host both hand it exactly two
    /// positional values off the air.
    #[allow(clippy::too_many_arguments)] // the field vector plus log_mode_at's own
    pub fn log_fields_at(
        &mut self,
        call: &str,
        fields: &[(String, String)],
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        self.log_fields_on(call, fields, mode, submode, slot, when_unix, None, None)
    }

    /// ⭐ **Log a contact worked THROUGH A BIRD** — [`log_fields_at`](Self::log_fields_at)
    /// for a satellite pass, and the ONE entry point whose caller names the band.
    ///
    /// ⚠️ Read [`SatLeg`] before changing what this does with `sat`. The whole of it is
    /// that a satellite QSO's band belongs to the transponder leg, not to wherever the
    /// radio is standing when the operator finally types the contact up — every other
    /// path here stamps [`band`](Self::band), and for a pass written up after LOS that
    /// is the wrong band in the contest log and in every file it reaches.
    ///
    /// `slot` is 0: a pass contact has no FT slot, exactly as the manual cockpit entries
    /// have none.
    pub fn log_satellite_at(
        &mut self,
        call: &str,
        fields: &[(String, String)],
        mode: &str,
        submode: &str,
        sat: &SatLeg,
        when_unix: u64,
    ) -> bool {
        self.log_fields_on(call, fields, mode, submode, 0, when_unix, Some(sat), None)
    }

    /// ⭐ **The contest ENTRY STRIP's write** — [`log_fields_at`](Self::log_fields_at), or
    /// [`log_satellite_at`](Self::log_satellite_at) with a pass `sat`, carrying the operator's
    /// own sent RST for this one contact when they typed one in the strip's optional "RST sent"
    /// box (`sent_rst`, already checked with
    /// [`ContestSession::sent_rst_ok`](crate::contest::ContestSession::sent_rst_ok)). With
    /// `None` the row is exactly what those two write. `slot` is 0: a typed contact has no FT
    /// slot.
    #[allow(clippy::too_many_arguments)] // log_satellite_at's own, plus the report
    pub fn log_entry_at(
        &mut self,
        call: &str,
        fields: &[(String, String)],
        mode: &str,
        submode: &str,
        when_unix: u64,
        sat: Option<&SatLeg>,
        sent_rst: Option<&str>,
    ) -> bool {
        self.log_fields_on(call, fields, mode, submode, 0, when_unix, sat, sent_rst)
    }

    /// The shared body of the three above — the field vector resolved against the running
    /// exchange, then the one write site.
    #[allow(clippy::too_many_arguments)] // log_fields_at's own, plus the leg and the report
    fn log_fields_on(
        &mut self,
        call: &str,
        fields: &[(String, String)],
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
        sat: Option<&SatLeg>,
        sent_rst: Option<&str>,
    ) -> bool {
        let spec = self.session.exchange;
        let rx: Vec<crate::contest::FieldValue> = fields
            .iter()
            .filter_map(|(k, v)| spec.copied(k, v))
            .collect();
        let mut tx = self.session.tx_for_row(call, mode);
        if let Some(raw) = sent_rst {
            self.session.override_sent_rst(&mut tx, raw);
        }
        self.log_exchange_on(call, rx, tx, mode, submode, slot, when_unix, sat)
    }

    /// A received Field Day exchange as a field vector, resolved against the exchange
    /// this session runs.
    ///
    /// ⚠️ The raw values are stored VERBATIM. Uppercasing here would rewrite what a
    /// shipped log holds and move the §8(a) goldens; normalisation belongs to whatever
    /// copied the value off the air.
    fn received(&self, class: &str, section: &str) -> Vec<crate::contest::FieldValue> {
        let spec = self.session.exchange;
        ["CLASS", "SECTION"]
            .iter()
            .zip([class, section])
            .filter_map(|(k, v)| spec.value(k, v))
            .collect()
    }

    /// ⭐ **The rows that COUNT** — every row except the duplicates, which are reported
    /// to the sponsor and are worth nothing to us.
    ///
    /// ⚠️ **Every number the operator CLAIMS is derived through here**, and that is the
    /// whole safety property of logging dupes at all: the row goes in the file, and no
    /// count, point, multiplier or board cell moves. For a log that holds no dupe this
    /// yields the whole log, and every number below is bit-identical to what it always
    /// was.
    ///
    /// ⭐ **A contact in a mode the contest bans is left out the same way** (FT8 at the
    /// Illinois QSO Party: *"FT4 and FT8 contacts will receive no contact credit"*): logged,
    /// in the files, and worth no point, multiplier, bonus or board cell
    /// ([`LoggedQso::banned_under`]). It still keys the dupe index like any row, so whether a
    /// later contact is a duplicate is what it always was, and the FT sequencer, which asks
    /// that index whether a station was worked, is not touched by it.
    ///
    /// ⚠️ **The hosted Remote page recounts [`qso_count`](Self::qso_count) from the rows it
    /// is sent**, by the marks a row carries (its `dupe`, its `sat` when the status says
    /// the contest gives a satellite no credit, and its `banned`), and refuses a capture whose
    /// count it cannot reproduce. A row left out here for any other reason has to carry a
    /// mark the page reads first, or that page's contest view goes blank.
    ///
    /// `+ Clone` because [`score_rows`](Self::score_rows) must stay clonable — the
    /// scorer walks its rows twice, once for points and once for multipliers.
    fn counting(&self) -> impl Iterator<Item = &LoggedQso> + Clone {
        let rs = self.ruleset();
        self.qsos.iter().filter(move |q| counts_under(rs, q))
    }

    /// ⭐ **Does this row COUNT?** The one predicate [`counting`](Self::counting) filters by,
    /// for a surface that walks [`qsos`](Self::qsos) itself and must leave out exactly the rows
    /// the score does: the club TV board, which attributes each counting contact to the
    /// position that logged it.
    pub fn counts(&self, q: &LoggedQso) -> bool {
        counts_under(self.ruleset(), q)
    }

    /// ⭐ **The rows the entry SUBMITS** — every row except a satellite contact the ruleset
    /// gives no credit, which the sponsor says not to log (Winter Field Day 2027 rules, p.5).
    /// Duplicates stay: a contest that reports them wants them in the file. The journal and
    /// the general-log merge read [`qsos`](Self::qsos), every row, so the contact is never
    /// lost; only the submitted files leave it out.
    fn submitted(&self) -> impl Iterator<Item = &LoggedQso> + Clone {
        let credit = self.ruleset().satellite_credit;
        self.qsos.iter().filter(move |q| credit || q.sat.is_none())
    }

    /// ⭐ **The RAW NON-DUPE contact count** — the number a summary sheet claims, not
    /// the number of rows in the file.
    ///
    /// ⚠️ The two differ only for a contest that reports its duplicates, and keeping
    /// them apart is the point: Field Day's own deliverable is a dupe sheet plus "raw
    /// non-dupe" counts, so a dupe that entered this count would overstate a submitted
    /// score. [`qsos`](Self::qsos) is the other question — every row, dupes included,
    /// which is what an exporter and the log table want.
    pub fn qso_count(&self) -> usize {
        self.counting().count()
    }

    /// The live rows, in log order. A contact the operator removed is not here; it is in
    /// [`removed`](Self::removed).
    pub fn qsos(&self) -> &[LoggedQso] {
        &self.qsos
    }

    /// Mutable rows — **tests only**, and deliberately not a public API.
    ///
    /// Every production path into this log stamps a club-sync sequence (log time
    /// stamps one; a journal restore backfills one, and `filter(|&v| v > 0)` means even
    /// a journaled zero backfills). That is exactly why the merge's defence against an
    /// unstamped row cannot be reached from outside: the shape it refuses has to be
    /// built by hand.
    #[cfg(test)]
    pub(crate) fn qsos_mut(&mut self) -> &mut [LoggedQso] {
        &mut self.qsos
    }

    /// This log's rows as the scorer reads them.
    ///
    /// The seam that took scoring off this type: `Scoring::qso_and_powered` used to
    /// take a `&FieldDayLog`, which is what welded the scoring math to Field Day's
    /// own log. Borrowed and lazy, because the score is recomputed on every
    /// snapshot tick and must stay O(rows) with no allocation.
    /// ⚠️ `+ Clone` is load-bearing: [`Scoring::score`](crate::contest::Scoring::score)
    /// walks these rows TWICE — once for points, once for multipliers — and a
    /// once-through iterator would force it to collect a `Vec` on every snapshot tick.
    ///
    /// ⭐ **Duplicates are not here.** They are rows in the log and lines in the
    /// Cabrillo, but they earn no point and no multiplier, so the scorer is never shown
    /// one ([`counting`](Self::counting)). Filtering at this seam rather than inside
    /// `Scoring` is what keeps the scorer a pure function of the rows it is handed and
    /// keeps "what a dupe is worth" a single statement in one place.
    pub fn score_rows(&self) -> impl Iterator<Item = crate::contest::ScoreRow<'_>> + Clone {
        let mine = self.session.my_call_location;
        self.counting().map(move |q| crate::contest::ScoreRow {
            mode_class: &q.mode,
            band: &q.band,
            role: &q.role,
            rx: &q.rx,
            entity: q.entity.as_deref(),
            prefix: q.prefix.as_deref(),
            // ⭐ The relation is computed HERE because this is the one place both sides
            // are in hand: the session knows where I am, the row knows where they were.
            // Neither the scorer (which has no session) nor the row (which has no `me`)
            // can answer it alone.
            relation: crate::contest::Relation::between(
                mine.as_ref().map(|l| l.as_pair()),
                q.entity.as_deref().zip(q.continent.as_deref()),
            ),
        })
    }

    /// ⭐ **Points this log EARNED from the contest's bonus stations** — once per
    /// station, whatever the band or mode
    /// ([`FdRuleset::bonus_station_points`](crate::fd_rules::FdRuleset::bonus_station_points)).
    ///
    /// Separate from the ticked bonus menu (`FdRuleset::bonus_points`) because the two
    /// answer different questions: that one is what the operator CLAIMED, this one is
    /// what the log holds. 0 for every ruleset that declares no bonus station, which is
    /// every contest but the Illinois QSO Party.
    pub fn bonus_station_points(&self) -> u32 {
        self.ruleset()
            .bonus_station_points(self.counting().map(|q| q.call.as_str()))
    }

    /// Distinct ARRL/RAC sections worked — a DISPLAY count for the worked-sections
    /// board, **not a multiplier**: neither Field Day event has one, and no scoring
    /// path reads this. (Said otherwise here until 2026-09-08, on the very function
    /// a reader would check.)
    pub fn sections(&self) -> usize {
        self.counting()
            .map(|q| q.section())
            .collect::<HashSet<_>>()
            .len()
    }

    /// The distinct sections worked — the identities behind the
    /// [`sections`](Self::sections) count, sorted for a stable board order
    /// (the worked-sections color board, spec §5).
    pub fn worked_sections(&self) -> Vec<String> {
        let mut sections: Vec<String> = self
            .counting()
            .map(|q| q.section().to_string())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        sections.sort();
        sections
    }

    /// The distinct values RECEIVED in one exchange slot, sorted — the generalisation
    /// of [`worked_sections`](Self::worked_sections) that one block of the multiplier
    /// display (§9) colours in.
    ///
    /// Blanks are dropped. A board colours cells by code, so a blank matches nothing
    /// either way; dropping it keeps a legacy row with no value out of a count that
    /// would otherwise read one too high.
    pub fn worked_values(&self, slot: &str) -> Vec<String> {
        let mut vals: Vec<String> = self
            .counting()
            .map(|q| q.rcvd(slot))
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        vals.sort();
        vals
    }

    /// Per-mode QSO points (phone 1, CW/digital 2) — power multiplier and
    /// bonuses are applied at the score layer (engine), not here.
    pub fn qso_points(&self) -> u32 {
        self.counting().map(|q| qso_points_for_mode(&q.mode)).sum()
    }

    /// ⭐ **Per-operator statistics** — the contest's counting rows grouped by the operator
    /// each was stamped with ([`LoggedQso::operator`]), best first (points, then contacts,
    /// then call). Dupes, banned modes and uncredited satellite rows are left out exactly as
    /// the score leaves them out ([`counting`](Self::counting)); removed contacts are not in
    /// the live log at all. Points and new multipliers come from the ruleset's own scorer, so
    /// the operators' figures sum to the log's QSO points and multiplier count.
    ///
    /// One pass over the log; `now_unix` is the caller's, so the rate windows are testable.
    pub fn operator_stats(&self, now_unix: u64) -> Vec<OperatorStats> {
        use std::collections::HashMap;
        let scoring = &self.ruleset().scoring;
        let new_mults = scoring.new_mults_by_row(self.score_rows());
        let mut stats: Vec<OperatorStats> = Vec::new();
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut hours: Vec<HashMap<u64, u32>> = Vec::new();
        let mut cells: Vec<HashMap<(&str, &str), u32>> = Vec::new();
        for ((q, row), new) in self.counting().zip(self.score_rows()).zip(new_mults) {
            let i = *index.entry(q.operator.as_str()).or_insert_with(|| {
                stats.push(OperatorStats {
                    operator: q.operator.clone(),
                    ..OperatorStats::default()
                });
                hours.push(HashMap::new());
                cells.push(HashMap::new());
                stats.len() - 1
            });
            let s = &mut stats[i];
            s.qsos += 1;
            s.points += scoring.qso_points.points_for(&row);
            s.new_mults += new;
            let w = q.when_unix;
            if w <= now_unix && w + 600 > now_unix {
                s.last_10_min += 1;
            }
            if w <= now_unix && w + 3600 > now_unix {
                s.last_60_min += 1;
            }
            *hours[i].entry(w - w % 3600).or_default() += 1;
            *cells[i]
                .entry((q.band.as_str(), q.mode.as_str()))
                .or_default() += 1;
        }
        for ((s, h), c) in stats.iter_mut().zip(hours).zip(cells) {
            s.best_hour = h
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                .unwrap_or_default();
            s.band_mode = c
                .into_iter()
                .map(|((b, m), n)| (b.to_string(), m.to_string(), n))
                .collect();
            s.band_mode.sort();
        }
        stats.sort_by(|a, b| {
            b.points
                .cmp(&a.points)
                .then(b.qsos.cmp(&a.qsos))
                .then(a.operator.cmp(&b.operator))
        });
        stats
    }

    /// ⭐ **The multipliers a spotted station would add** ([`SpotMults`]): what this log has
    /// earned, under the scorer's own keys, and each call's newest received exchange.
    pub fn spot_mults(&self) -> SpotMults {
        let ruleset = self.ruleset();
        let mut rx_by_call = std::collections::HashMap::new();
        for q in &self.qsos {
            rx_by_call.insert(q.call.trim().to_ascii_uppercase(), q.rx.clone());
        }
        let role = self.session.role();
        let exchange = self.session.exchange;
        SpotMults {
            ruleset,
            worked: ruleset.scoring.mults_worked(self.score_rows()),
            role: role.id.to_string(),
            receives: role
                .receives
                .iter()
                .filter_map(|k| exchange.field(k))
                .collect(),
            rx_by_call,
        }
    }

    /// Export the log as ADIF records (one `<EOR>` per QSO): every live row, then every
    /// removed one marked `APP_NEXUS_REMOVED` with when it was removed — what the club's
    /// export is built from, and the rows the journal writes
    /// ([`journal_adif`](Self::journal_adif)), which gives each one its sent exchange as well.
    ///
    /// A row that sent a report writes it as `RST_SENT`, the tag another program reads, as the
    /// submitted file's rows do ([`submission_adif`](Self::submission_adif)): for a club this is
    /// the file its entry submits. The journal writes none.
    ///
    /// ⚠️ **Live rows first, and that order is the downgrade's.** A build that predates the
    /// tag reads a removed row as an ordinary contact (the safe direction: nothing is lost),
    /// and admits rows in file order — so where a removed contact's station was worked again,
    /// the LIVE row reaches its dupe index first and the old build keeps the live log this
    /// one has, passing over the removed row instead of the reverse.
    pub fn adif(&self) -> String {
        let removed = self
            .removed
            .iter()
            .flat_map(|r| r.rows.iter().map(move |q| (q, Some(r.removed_unix))));
        // A removed row that alone sent a moved exchange still needs every row's carrier, or
        // a restart would give it the session's exchange and lose the serial it was sent.
        let sent_moved = self
            .qsos
            .iter()
            .chain(self.removed.iter().flat_map(|r| &r.rows))
            .any(|q| q.tx != self.session.my_exchange);
        self.adif_of(
            "",
            self.qsos.iter().map(|q| (q, None)).chain(removed),
            sent_moved,
            true,
            false,
            None,
        )
    }

    /// ⭐ **The ADIF the entry submits**: the live rows (a removed contact is not submitted)
    /// without the satellite contacts a ruleset gives no credit (Winter Field Day: *"Do not
    /// log any such contacts"*, 2027 rules p.5). [`adif`](Self::adif), the club's export, keeps
    /// every row.
    pub fn submission_adif(&self) -> String {
        let sent_moved = self.qsos.iter().any(|q| q.tx != self.session.my_exchange);
        self.adif_of(
            "",
            self.submitted().map(|q| (q, None)),
            sent_moved,
            true,
            false,
            Some(&self.qso_numbers()),
        )
    }

    /// `header`: the journal's own header fields, before `<EOH>` (an export has none).
    ///
    /// ⭐ `sent_moved`: EVERY row gets its own `APP_NEXUS_MYEX`, not just the ones that
    /// differ from the session, so a later move cannot re-label the rows that happen to match
    /// it today. The journal always asks for it ([`journal_adif`](Self::journal_adif));
    /// [`adif`](Self::adif) and the submitted file once the sent exchange has moved in the rows
    /// they hold.
    ///
    /// `rst_sent`: each row's own sent report under ADIF's `RST_SENT`, the tag another program
    /// reads. Both exports ask for it, the submitted file
    /// ([`submission_adif`](Self::submission_adif)) and the club's ([`adif`](Self::adif)). The
    /// journal does not: a restore reads the sent side from `APP_NEXUS_MYEX`, so the journal's
    /// bytes stay as they were.
    ///
    /// `journal`: the journal's own rows, which also carry the call each was logged on
    /// (`APP_NEXUS_ONAIR`, [`LoggedQso::on_air_call`]). No export writes it.
    ///
    /// `numbers`: each row's QSO number under ADIF's `STX` ([`qso_numbers`](Self::qso_numbers)),
    /// the serial it was sent in a serial contest. Only the submitted file asks for it: the
    /// journal never stores a number it can read off the rows, and the club's export is pinned
    /// byte for byte by the Field Day goldens.
    fn adif_of<'a>(
        &'a self,
        header: &str,
        rows: impl Iterator<Item = (&'a LoggedQso, Option<u64>)>,
        sent_moved: bool,
        rst_sent: bool,
        journal: bool,
        numbers: Option<&std::collections::HashMap<u64, u32>>,
    ) -> String {
        let mut s = format!("ADIF Export from Nexus\n<PROGRAMID:5>Nexus\n{header}<EOH>\n");
        for (q, removed_unix) in rows {
            s.push_str(&adif_field("CALL", &q.call));
            // ⚠️ A MODE OUTSIDE ADIF'S ENUMERATION IS A DROPPED RECORD, NOT A COSMETIC ONE.
            // This wrote `q.submode` raw, and the tiers stamp names ADIF has never heard of
            // ("TempoFast", "TempoDeep", "FT2") — the main logbook already solved that with
            // `logbook::adif_submode`, whose own comment spells out the cost: a bare
            // <MODE:3>FT2 misses all three legs of TQSL's MODE%SUBMODE → SUBMODE → MODE
            // cascade and the record is DROPPED with "Invalid MODE". The Field Day exporter
            // simply never called it, so a Field Day contact worked on a Tempo tier was lost
            // on upload. Same cascade now, so the two exports cannot disagree.
            let recorded = q.recorded_mode();
            match crate::logbook::adif_submode(recorded) {
                Some((parent, sub)) => {
                    s.push_str(&adif_field("MODE", parent));
                    s.push_str(&adif_field("SUBMODE", sub));
                }
                None => s.push_str(&adif_field("MODE", recorded)),
            }
            s.push_str(&adif_field("BAND", &q.band));
            // The dial, when this row knows it — in MHz, as ADIF's FREQ is. A row that does
            // not writes nothing rather than a zero an importer would refuse the record
            // over.
            //
            // ⚠️ **FIELD DAY IS NOT EXCEPTED, AND THE EXCEPTION THAT USED TO SIT HERE WAS
            // JUSTIFIED BY A CLAIM THAT IS NOT TRUE.** It read "never for Field Day: its
            // journal and export are pinned byte for byte by the §8(a) goldens". They are
            // not: every row in the golden fixtures carries `dial_khz == 0`, so all ten
            // pass with the clause or without it, and they never held it in place. Measured,
            // then removed (operator ruling, 2026-09-20).
            //
            // What the exception actually cost: this function is BOTH the journal and the
            // submitted export, and [`Self::merge_adif`] restores `freq_khz` by reading
            // `FREQ` back out of it — so withholding the field here silently zeroed the dial
            // on every already-logged contact the moment Nexus restarted mid-event, and
            // nothing told the operator. Pinned by
            // `a_field_day_contact_keeps_its_dial_across_a_restart`.
            // ⭐ ADIF `FREQ` is the frequency RADIATED, so it is the on-air value and not the
            // dial — six decimals, the general log's own precision, because rounding to the
            // kHz Cabrillo writes in would throw away the audio offset this exists to carry.
            // A row that predates the pair falls back to the dial, which is what its `FREQ`
            // meant when it was written.
            let journal_mhz = if q.on_air_hz > 0 {
                q.on_air_hz as f64 / 1e6
            } else {
                f64::from(q.freq_khz) / 1000.0
            };
            if journal_mhz > 0.0 {
                s.push_str(&adif_field("FREQ", &format!("{journal_mhz:.6}")));
            }
            if let Some(rx) = q.freq_rx_hz.filter(|hz| *hz > 0) {
                s.push_str(&adif_field("FREQ_RX", &format!("{:.6}", rx as f64 / 1e6)));
            }
            // ⭐ THE SATELLITE PAIR — ADIF `PROP_MODE` + `SAT_NAME`, **both or neither**.
            // TQSL validates them as a pair and hard-errors on a lone member, which via
            // the `-a compliant` funnel wedges the whole upload batch as Rejected, so
            // this writer can only ever produce the pair: a leg with no LoTW-creditable
            // designator writes nothing at all. Nothing moves for a terrestrial row, so
            // the §8(a) goldens are untouched. `merge_adif` reads both back.
            if let Some(name) = q.sat.as_ref().and_then(|s| s.name.as_deref()) {
                s.push_str(&adif_field("PROP_MODE", "SAT"));
                s.push_str(&adif_field("SAT_NAME", name));
            }
            // ⭐ AND THE DUPE IDENTITY, which the standard pair cannot give back.
            // `SAT_NAME` is the LoTW designator and is absent for a bird LoTW does not
            // list; the key needs the bird either way, and it needs to know a
            // single-channel FM satellite from a linear transponder because ARRL limits
            // one to a single QSO per station. Without these a mid-event restart brings
            // the rows back with no bird on them: the satellite dimension collapses,
            // and the restored log both re-admits a repeat and blocks a legal
            // terrestrial contact on the same band. Private tags, written only for a
            // satellite row, so the §8(a) goldens do not move.
            if let Some(sat) = q.sat.as_ref() {
                if !sat.bird.trim().is_empty() {
                    s.push_str(&adif_field("APP_NEXUS_SATBIRD", sat.bird.trim()));
                }
                if sat.single_channel_fm {
                    s.push_str(&adif_field("APP_NEXUS_SATFM", "1"));
                }
            }
            // ⚠️ THE CABRILLO DIAL RIDES SEPARATELY, because `FREQ` no longer carries it.
            // `merge_adif` restores this, and without it a mid-event restart would rebuild
            // every row's dial from the on-air value and hand the sponsor a QSO line off by
            // the audio offset. An old journal has no such tag and falls back to `FREQ`.
            if q.freq_khz > 0 {
                s.push_str(&adif_field("APP_NEXUS_DIALKHZ", &q.freq_khz.to_string()));
            }
            // A real date/time so [`merge_adif`](Self::merge_adif) can restore
            // `when_unix` (and Cabrillo keeps its ARRL-required timestamps
            // across a restart). Legacy rows without a stamp omit both fields
            // rather than inventing a date.
            if q.when_unix > 0 {
                let (date, time) = adif_datetime(q.when_unix);
                s.push_str(&adif_field("QSO_DATE", &date));
                s.push_str(&adif_field("TIME_ON", &time));
            }
            // The SESSION's Cabrillo token, not the `FdEvent`'s: a QSO party has no
            // `FdEvent` arm, and reading one would export every party as Field Day.
            s.push_str(&adif_field("CONTEST_ID", &self.session.contest_id));
            // The RECEIVED exchange under its own slots' ADIF tags, in the role's
            // receive order — for Field Day exactly the <CLASS> and <ARRL_SECT> this
            // has always written, from the row instead of from two named columns.
            let spec = self.session.exchange;
            let role = crate::contest::role_for(q, spec);
            let standard: Vec<(&'static str, String)> = standard_rcvd_slots(spec, role)
                .map(|(key, tag)| (tag, q.rcvd(key).to_string()))
                .collect();
            for (tag, val) in &standard {
                s.push_str(&adif_field(tag, val));
            }
            // ⭐ The report this row SENT, read off the row by the merged Logbook's own function,
            // so the two files cannot disagree: never the session's report, and nothing for a
            // row that sent none or an exchange that has none (both Field Day events).
            if rst_sent {
                let sent = crate::contest::sent_exchange(q, spec);
                if let Some(rst) = crate::contest::merge::rst_of(&sent, spec) {
                    s.push_str(&adif_field("RST_SENT", &rst));
                }
            }
            if let Some(n) = numbers.and_then(|by_seq| by_seq.get(&q.seq)) {
                s.push_str(&adif_field("STX", &n.to_string()));
            }
            // ⭐ The private carrier rides ONLY when the standard tags cannot give the
            // row back — and that condition is not a guess, it is the restore run
            // forward: `rx_from_standard` is the SAME function `restore_row` falls back
            // to, so "the fallback is exact" and "no tag is written" are one fact and
            // cannot drift. For Field Day it is exact (class and section ARE the received
            // exchange), so nothing is written here. The sent side has no such test: its
            // fallback is the restoring session's exchange, which Settings can change under
            // a row already written, so the journal writes it on every row (`sent_moved`).
            if rx_from_standard(spec, role, |tag| {
                standard
                    .iter()
                    .find(|(t, _)| *t == tag)
                    .map(|(_, v)| v.clone())
            }) != q.rx
            {
                s.push_str(&adif_field(
                    "APP_NEXUS_EX",
                    &crate::contest::carrier::encode(&q.rx),
                ));
            }
            if sent_moved {
                s.push_str(&adif_field(
                    "APP_NEXUS_MYEX",
                    &crate::contest::carrier::encode(&q.tx),
                ));
            }
            // Same rule for the role: written only when it is not the session's own,
            // which for a symmetric contest (both Field Day events) is never.
            if q.role != self.session.role().id {
                s.push_str(&adif_field("APP_NEXUS_ROLE", &q.role));
            }
            // The per-position sync sequence (APP_-namespaced per the ADIF
            // spec, so every other consumer ignores it). Legacy 0 rows omit
            // the tag — restore backfills them in row order.
            if q.seq > 0 {
                s.push_str(&adif_field("APP_NEXUS_QSEQ", &q.seq.to_string()));
            }
            // ADIF's own `OPERATOR` ("the logging operator's callsign"), the tag the general
            // log already writes for the same person. Only when somebody was named.
            if !q.operator.is_empty() {
                s.push_str(&adif_field("OPERATOR", &q.operator));
            }
            // The call it was logged on, in the journal alone: a restart reads every row into a
            // session opened under whatever call Settings holds then.
            if journal && !q.on_air_call.is_empty() {
                s.push_str(&adif_field("APP_NEXUS_ONAIR", &q.on_air_call));
            }
            // The radio and the power it was made with, in the journal, under their ADIF names:
            // the merge carries them into the Logbook, and a restart must not lose them.
            if journal && !q.rig.is_empty() {
                s.push_str(&adif_field("MY_RIG", &q.rig));
            }
            if journal && !q.tx_power.is_empty() {
                s.push_str(&adif_field("TX_PWR", &q.tx_power));
            }
            // …and the grid it was made from, under a private tag: a VHF contest's sent `GRID`
            // already writes `MY_GRIDSQUARE` on this record.
            if journal && !q.grid.is_empty() {
                s.push_str(&adif_field("APP_NEXUS_MYGRID", &q.grid));
            }
            // A contact the operator removed: kept, with when (`restore_row` reads it back).
            if let Some(at) = removed_unix {
                s.push_str(&adif_field("APP_NEXUS_REMOVED", &at.to_string()));
            }
            s.push_str("<EOR>\n");
        }
        s
    }

    /// Merge a previously-flushed ADIF journal (see [`adif`](Self::adif)) back
    /// into this log — the restore half of the durable Field Day backup, so a
    /// restart mid-event doesn't reset the contest log. Rows missing a CALL or
    /// stamped before `min_when_unix` are skipped (a previous event's journal
    /// stays out of the log; the second are kept for the journal, [`aged`](Self::aged)),
    /// a row already in this log by its sequence number is skipped, and garbage
    /// input merges nothing — never an error. Restored dupe keys keep the ROW's
    /// band, so they survive a mid-event QSY. A row the journal marks removed
    /// (`APP_NEXUS_REMOVED`) comes back REMOVED ([`removed`](Self::removed)), outside the
    /// dupe index, with its seq still counted, and its serial unless a clear took it out
    /// (`cleared`).
    ///
    /// Returns what it found ([`AdifMerge`]): the rows it restored, and the records it could not
    /// read — no CALL, or cut off before their `<EOR>` — which the journal's reader keeps the
    /// file for, because the next contact rewrites the journal from this log without them.
    pub fn merge_adif(&mut self, text: &str, min_when_unix: u64) -> AdifMerge {
        self.merge_journal(text, min_when_unix, None)
    }

    /// [`merge_adif`](Self::merge_adif), holding the records of another contest or of
    /// another running than `running` when one is given ([`restore_journal`](Self::restore_journal)).
    fn merge_journal(&mut self, text: &str, min_when_unix: u64, running: Option<u16>) -> AdifMerge {
        // Minimal `<NAME:len>value` tokenizer mirroring logbook.rs `parse_adif`
        // (this journal only needs the handful of FD tags).
        let body = match text.to_ascii_uppercase().find("<EOH>") {
            Some(i) => {
                // The numbers of the rows an earlier restore let go (`spent`).
                self.spent = self.spent.max(Spent::from_header(&text[..i]));
                // The latest clear (`cleared`), as the journal holds it: never a mix of two.
                if let Some(cleared) = Cleared::from_header(&text[..i]) {
                    self.cleared = cleared;
                }
                &text[i + 5..]
            }
            None => text,
        };
        let mut cur: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut merged = AdifMerge::default();
        let mut torn = false;
        // Every record kept past the window, by its text, so a journal read again into this log
        // keeps each once without a pass over all of them per record: a year of contests is tens
        // of thousands, and the restore runs under the Engine lock.
        let already: Vec<String> = self.aged.iter().map(|a| a.adif.clone()).collect();
        let mut kept: HashSet<&str> = already.iter().map(String::as_str).collect();
        let bytes = body.as_bytes();
        let mut i = 0;
        // Where the record being read began: just past the last `<EOR>`.
        let mut record_start = 0;
        while i < bytes.len() {
            if bytes[i] != b'<' {
                i += 1;
                continue;
            }
            let end = match body[i..].find('>') {
                Some(e) => i + e,
                None => {
                    torn = true;
                    break;
                }
            };
            let tag = &body[i + 1..end];
            i = end + 1;
            if tag.eq_ignore_ascii_case("EOR") {
                // ⭐ The restore loads this session's records whatever their age. Another
                // contest's, or another running's, is held while it is inside the window, and
                // past it is kept until the Logbook holds a copy (`restore_row` ages it).
                let held = running.and_then(|r| self.kept_out(&cur, r));
                let window = if running.is_some() && held.is_none() {
                    0
                } else {
                    min_when_unix
                };
                match held.filter(|_| record_when(&cur) >= window) {
                    Some(why) => self.hold(why, body[record_start..i].trim_start(), &cur),
                    None => match self.restore_row(&cur, window) {
                        RowFate::Restored => merged.restored += 1,
                        RowFate::NoCall => merged.unreadable += 1,
                        RowFate::Passed => {}
                        RowFate::Aged => {
                            let record = body[record_start..i].trim_start();
                            if kept.insert(record) {
                                self.keep_aged(record, &cur);
                            }
                        }
                    },
                }
                cur.clear();
                record_start = i;
                continue;
            }
            // NAME:len or NAME:len:type
            let mut parts = tag.splitn(3, ':');
            let name = parts.next().unwrap_or("").to_ascii_uppercase();
            let len: usize = parts
                .next()
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or(0);
            let val = body.get(i..i + len).unwrap_or("").to_string();
            i += len;
            cur.insert(name, val);
        }
        // A record cut off before its `<EOR>`: the tail a torn write leaves.
        if torn || !cur.is_empty() {
            merged.unreadable += 1;
        }
        merged
    }

    /// ⭐ **The journal restore: [`merge_adif`](Self::merge_adif) of this session's rows
    /// only** — the ones of this log's own contest (`CONTEST_ID`), logged in the running of
    /// it that `now_unix` belongs to ([`FdRuleset::running_of`](crate::fd_rules::FdRuleset::running_of)).
    ///
    /// The journal is one file per position, whatever contest it last ran, and the restore
    /// used to load every row of the last four days into the session starting: a club that
    /// rehearsed on the Wednesday began the Sunday's party with the rehearsal's contacts in
    /// every position's log, and a position that had run another contest that week read its
    /// rows against the party's exchange. A restart inside the session — a crash, a relaunch,
    /// the day after to export — still restores every one of its own rows.
    ///
    /// ⚠️ **Nothing is deleted.** The rows kept out are HELD, verbatim
    /// ([`held`](Self::held) counts them), and every rewrite of the journal carries them on
    /// ([`journal_adif`](Self::journal_adif)), so they are there for the session they belong
    /// to. Their sequence numbers stay taken: a club host or the general log holding a
    /// rehearsal's `(position, seq)` would read a re-used one as a contact it already has.
    ///
    /// ⭐ **This session's own rows are loaded whatever their age**: a sponsor takes a log for
    /// weeks, so the Cabrillo file, the score and the dupe check of a session opened days
    /// after the contest are the whole running's. `min_when_unix` ages only the records kept
    /// out: one of another contest or running older than it is not held, but kept the same way
    /// until a copy of it is in the Logbook ([`aged`](Self::aged)), its number spent.
    ///
    /// ⭐ **The numbers in the journal's header still count** (`spent`): an earlier build's
    /// restore let this running's rows past its four days go, and a contact logged now numbers
    /// on from them. Only this running's: the next one's names never meet them, and a club
    /// host starting that event empty would count numbers carried on as contacts still to
    /// come.
    pub fn restore_journal(&mut self, text: &str, min_when_unix: u64, now_unix: u64) -> AdifMerge {
        let running = self.ruleset().running_of(now_unix);
        let merged = self.merge_journal(text, min_when_unix, Some(running));
        // The latest row let go decides. When it is not this running's, none of them is.
        if self.spent.seq > 0 && self.ruleset().running_of(self.spent.when_unix) == running {
            self.next_seq = self.next_seq.max(self.spent.seq + 1);
        }
        merged
    }

    /// How many journal records the restore kept out: `(another contest, another running
    /// of this one)`.
    pub fn held(&self) -> (usize, usize) {
        let count = |why| self.held.iter().filter(|h| h.why == why).count();
        (count(Held::OtherContest), count(Held::OtherRunning))
    }

    /// ⭐ **The journal: the rows of [`adif`](Self::adif), each with the exchange it was sent
    /// with; in its header the numbers of the rows a restore let go (`spent`) and the latest
    /// clear (`cleared`); and the records the restore held, then the ones it kept past its
    /// window** ([`aged`](Self::aged)), after them, as the journal held them — what the engine
    /// writes the journal file from, so a row kept out of this session is never dropped by a
    /// rewrite. Never an export: those carry this log's rows alone, as [`adif`](Self::adif)
    /// writes them.
    pub fn journal_adif(&self) -> String {
        let removed = self
            .removed
            .iter()
            .flat_map(|r| r.rows.iter().map(move |q| (q, Some(r.removed_unix))));
        // ⭐ EVERY ROW CARRIES THE EXCHANGE IT WAS SENT WITH, moved or not. A row without one
        // is read back with the sent exchange of the session that restores it, and that
        // session is built from Settings: a County or section changed there — in the contest,
        // out of it, or with Nexus closed — put the new one on every row written without one,
        // and the Cabrillo file said so. This wrote it only once some row differed from the
        // session running now, which a Settings edit never makes true, and nothing here can
        // know what the next session will be built from.
        let mut s = self.adif_of(
            &(self.spent.header() + &self.cleared.header()),
            self.qsos.iter().map(|q| (q, None)).chain(removed),
            true,
            false,
            true,
            None,
        );
        for h in &self.held {
            s.push_str(&h.adif);
            s.push('\n');
        }
        for a in &self.aged {
            s.push_str(&a.adif);
            s.push('\n');
        }
        s
    }

    /// Why the restore keeps `f` out of this log: it is another contest's, or another
    /// running's of this one. `None` for one of this session's own, which the restore loads
    /// whatever its age (and for a record with no call, which
    /// [`restore_row`](Self::restore_row) reports).
    fn kept_out(
        &self,
        f: &std::collections::HashMap<String, String>,
        running: u16,
    ) -> Option<Held> {
        f.get("CALL").filter(|c| !c.trim().is_empty())?;
        let contest = f.get("CONTEST_ID").map_or("", |c| c.trim());
        if !contest.is_empty() && !contest.eq_ignore_ascii_case(self.session.contest_id.trim()) {
            return Some(Held::OtherContest);
        }
        (self.ruleset().running_of(record_when(f)) != running).then_some(Held::OtherRunning)
    }

    /// Keep a journal record out of this log, and keep it.
    fn hold(&mut self, why: Held, adif: &str, f: &std::collections::HashMap<String, String>) {
        if let Some(seq) = f
            .get("APP_NEXUS_QSEQ")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
        {
            self.next_seq = self.next_seq.max(seq + 1);
        }
        if !self.held.iter().any(|h| h.adif == adif) {
            self.held.push(HeldRecord {
                why,
                adif: adif.to_string(),
            });
        }
    }

    /// Keep a journal record older than the restore's window, as the journal held it
    /// ([`aged`](Self::aged)). Once: `merge_journal` passes over one already kept.
    fn keep_aged(&mut self, adif: &str, f: &std::collections::HashMap<String, String>) {
        self.aged.push(AgedRecord {
            adif: adif.to_string(),
            contest_id: f.get("CONTEST_ID").map_or("", |c| c.trim()).to_string(),
            when_unix: record_when(f),
            seq: f
                .get("APP_NEXUS_QSEQ")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0),
        });
    }

    /// How many contacts the journal keeps past its window ([`aged`](Self::aged)): the ones a
    /// Logbook has not yet been asked about ([`settle_aged`](crate::contest::settle_aged)), and
    /// the ones it holds no copy of.
    pub fn aged(&self) -> usize {
        self.aged.len()
    }

    /// ⭐ **The contacts the journal keeps past its window** ([`aged`](Self::aged)), in the
    /// journal's order, each with its contest and its merge identity at position `posid`.
    ///
    /// The identity is the one the merge would mint for the row in a session of its contest:
    /// this log's own contest and rules for a record of it (its `CONTEST_ID`, or none, as the
    /// restore reads one), the loaded ruleset of that id otherwise
    /// ([`ruleset_by_contest_id`](crate::fd_rules::ruleset_by_contest_id)) — the contest id and
    /// the rules a session built for that ruleset carries ([`aged_log`](Self::aged_log)).
    pub(crate) fn aged_contacts(&self, posid: &str) -> Vec<AgedContact> {
        self.aged
            .iter()
            .map(|a| {
                let ours = a.contest_id.is_empty()
                    || a.contest_id
                        .eq_ignore_ascii_case(self.session.contest_id.trim());
                let contest = if ours {
                    Some((self.session.contest_id.as_str(), self.ruleset()))
                } else {
                    crate::fd_rules::ruleset_by_contest_id(
                        &a.contest_id,
                        crate::fd_rules::CURRENT_RULES_YEAR,
                    )
                    .map(|rs| (rs.contest_id, rs))
                };
                AgedContact {
                    rules: contest.map(|(_, rs)| rs),
                    when_unix: a.when_unix,
                    identity: contest.filter(|_| a.seq > 0).map(|(id, rs)| {
                        crate::contest::MergeIdentity::of_contest(id, rs, posid, a.seq, a.when_unix)
                    }),
                }
            })
            .collect()
    }

    /// The contacts the journal keeps past its window at `at` in
    /// [`aged_contacts`](Self::aged_contacts), read back into one log on `session` — a session of
    /// their contest — as its live rows: what the merge offered for them writes from. Each is
    /// read alone, so that no other one's dupe key can turn it away: the sessions they were
    /// logged in never held them together.
    pub(crate) fn aged_log(&self, at: &[usize], session: crate::contest::ContestSession) -> Self {
        let mut log = Self::new(&self.mycall, session, &self.band);
        for a in at.iter().filter_map(|&i| self.aged.get(i)) {
            let mut one = Self::new(&self.mycall, log.session.clone(), &self.band);
            one.merge_adif(&a.adif, 0);
            log.qsos.append(&mut one.qsos);
        }
        log
    }

    /// Let go of the contacts the journal keeps past its window at `at` in
    /// [`aged_contacts`](Self::aged_contacts): the next rewrite of the journal leaves them out.
    /// Their numbers stay spent.
    pub(crate) fn let_go_aged(&mut self, at: &[usize]) {
        let at: HashSet<usize> = at.iter().copied().collect();
        let mut i = 0;
        self.aged.retain(|_| {
            i += 1;
            !at.contains(&(i - 1))
        });
    }

    /// A journal record the restore lets go, older than its window: its number stays taken
    /// ([`spent`](Self::spent)).
    fn spend(&mut self, f: &std::collections::HashMap<String, String>, when_unix: u64) {
        if let Some(seq) = f
            .get("APP_NEXUS_QSEQ")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
        {
            self.spent = self.spent.max(Spent { seq, when_unix });
        }
    }

    /// One tokenized journal record → the log (the dupe-checked insert half of
    /// [`merge_adif`](Self::merge_adif)).
    fn restore_row(
        &mut self,
        f: &std::collections::HashMap<String, String>,
        min_when_unix: u64,
    ) -> RowFate {
        let Some(call) = f.get("CALL").filter(|c| !c.trim().is_empty()) else {
            return RowFate::NoCall;
        };
        let (mode, submode) = class_of_adif(
            f.get("MODE").map(String::as_str),
            f.get("SUBMODE").map_or("", String::as_str),
        );
        let when_unix = record_when(f);
        let removed_unix = f
            .get("APP_NEXUS_REMOVED")
            .and_then(|v| v.trim().parse::<u64>().ok());
        if when_unix < min_when_unix {
            // Out of this log, and its number stays taken. A contact taken back goes with the
            // next rewrite of the journal, as every row past the window went; any other is kept.
            self.spend(f, when_unix);
            return match removed_unix {
                Some(_) => RowFate::Passed,
                None => RowFate::Aged,
            };
        }
        // The ROW's band, not the log's current one — a restored dupe key must
        // keep its original band across a mid-event QSY.
        let band = f.get("BAND").cloned().unwrap_or_default();
        let spec = self.session.exchange;
        // ⭐ The row's ROLE, then its two exchange sides. A 1.x journal carries none of
        // the three tags, and each falls back to what that build meant by its absence:
        // the session's role, the standard <CLASS>/<ARRL_SECT> columns, and the
        // session's current sent exchange. For Field Day all three are exact while Settings
        // still say what they said when the row was sent — which is why a 1.x journal
        // restores to a byte-identical log, and why the writer above emits no role and no
        // received side for a Field Day row. The third stops being exact the moment a class
        // or section changes in Settings, so the journal now writes every row's own sent
        // exchange (`journal_adif`) and this fallback reads only journals written before it.
        let role = f
            .get("APP_NEXUS_ROLE")
            .cloned()
            .unwrap_or_else(|| self.session.role().id.to_string());
        let role_spec = spec
            .roles
            .iter()
            .find(|r| r.id == role)
            .unwrap_or(self.session.role());
        let rx = match f.get("APP_NEXUS_EX") {
            Some(v) => crate::contest::carrier::decode(v, spec),
            None => rx_from_standard(spec, role_spec, |tag| f.get(tag).cloned()),
        };
        let tx = match f.get("APP_NEXUS_MYEX") {
            Some(v) => crate::contest::carrier::decode(v, spec),
            None => self.session.my_exchange.clone(),
        };
        // ⭐ **THE MARK IS RE-DERIVED HERE, never read from a tag** — the same rule the
        // serial run below is restored by, and for the same reason: a flag stored beside
        // the history that determines it is a second source of truth for one fact. It
        // also gets a case a tag would get wrong — when `min_when_unix` ages out the
        // FIRST occurrence, its survivor duplicates nothing that is still in this log and
        // is restored as the real contact.
        // ⭐ THE BIRD, BEFORE THE KEY. A restored row must be admitted under exactly the
        // rule the live one was, and for ARRL Field Day the bird is IN that key
        // (7.3.8) — restore it first and a mid-event restart re-derives the same marks;
        // restore it after and every satellite row keys as terrestrial, which both
        // re-admits a repeat through one bird and blocks a legal contact on its band.
        //
        // The LoTW designator is admitted only as half of a PAIR — a lone `PROP_MODE`
        // is not one, and neither is a lone `SAT_NAME`.
        let lotw = f
            .get("SAT_NAME")
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .filter(|_| {
                f.get("PROP_MODE")
                    .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"))
            });
        // The DUPE identity, which the standard pair cannot give back: `SAT_NAME` is
        // absent for a bird LoTW does not list, and the key still has to tell that
        // contact from one through another bird.
        let bird = f
            .get("APP_NEXUS_SATBIRD")
            .map(|b| b.trim())
            .filter(|b| !b.is_empty());
        let sat = (bird.is_some() || lotw.is_some()).then(|| SatLeg {
            band: band.clone(),
            // Filled below, where the journaled frequency is parsed.
            down_hz: 0,
            // Either tag alone still says "worked through a bird", so each falls back to
            // the other rather than leaving the row with no identity at all.
            bird: bird.or(lotw).unwrap_or_default().to_string(),
            name: lotw.map(str::to_string),
            single_channel_fm: f.get("APP_NEXUS_SATFM").is_some_and(|v| v.trim() == "1"),
        });
        let key = self.dupe_rule().key_of(
            call,
            &band,
            mode,
            &rx,
            &tx,
            sat.as_ref()
                .map(|s| crate::contest::SatKey {
                    bird: &s.bird,
                    single_channel_fm: s.single_channel_fm,
                })
                .unwrap_or_default(),
        );
        // The journaled sync seq round-trips; a legacy row without the tag
        // backfills the next free seq in row order (1..n on a whole legacy
        // journal), so pre-sync journals join the sequence deterministically.
        let journaled_seq = f
            .get("APP_NEXUS_QSEQ")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0);
        // ⭐ A REMOVED contact is outside the dupe check — that is what removing it means — so
        // it is never offered to the index. Offered, a station worked again since would refuse
        // it out of the journal right here, and the next save would drop it for good. Its
        // numbers are still read below, which is what stops a restart reissuing them.
        //
        // ⭐ **A live row already in this log by its number is this row, read again.** Every
        // contest logs its dupes, so the index admits a second reading of the same journal as
        // a second contact, marked; the row's sequence number is what tells the two apart (a
        // legacy row without one cannot be told apart, and is admitted as before).
        let dupe = match removed_unix {
            Some(_) => false,
            None if journaled_seq.is_some_and(|s| self.qsos.iter().any(|q| q.seq == s)) => {
                return RowFate::Passed;
            }
            None => match self.admit_row(key, sat.is_some()) {
                Some(dupe) => dupe,
                None => return RowFate::Passed,
            },
        };
        let seq = journaled_seq.unwrap_or(self.next_seq);
        self.next_seq = self.next_seq.max(seq + 1);
        // ⭐ **THE SERIAL RUN, advanced past this row exactly as `next_seq` is above —
        // and this is the half that goes ON THE AIR.** A re-used `seq` collides with a
        // club host's merged rows; a re-issued SERIAL is copied by the other operator
        // and lands in their log, so two of my QSO lines claim a number the checker can
        // only match to one of them.
        //
        // ⚠️ **RE-DERIVED from the rows, never persisted beside them**, and the reason
        // is that the issued number is ALREADY on the row (`APP_NEXUS_MYEX`, which every
        // journal row carries). A counter stored next to the history it numbers is a second
        // source of truth for one fact, and the two come apart exactly where it matters: a
        // journal that lost rows to `min_when_unix`, a partial restore, an operator who
        // edited the file. Reading the rows cannot drift from the rows.
        //
        // Only from the rows logged since the latest clear, which is a fact about an event
        // rather than a counter (`cleared`): a contact the clear took out numbers nothing,
        // restored or not, as in the live run.
        if let Some(n) =
            issued_serial(spec, &tx).filter(|_| !self.cleared.passed_over(seq, when_unix))
        {
            self.session.next_serial = self.session.next_serial.max(n + 1);
        }
        // Resolved on the RESTORE path too, and from the call the row carries — a
        // journal reload or an ADIF merge must score identically to the live log, and a
        // row restored with no entity would drop a country multiplier the operator
        // already worked.
        let placed = crate::contest::resolve_call(call);
        // The journaled dial (MHz), if the row carried one. Anything unparseable is simply
        // unknown — never a guessed frequency.
        let mhz_of = |key: &str| {
            f.get(key)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|mhz| mhz.is_finite() && *mhz > 0.0 && *mhz < 1_000_000.0)
        };
        let on_air_hz = mhz_of("FREQ").map_or(0, |mhz| (mhz * 1e6).round() as u64);
        let freq_rx_hz = mhz_of("FREQ_RX").map(|mhz| (mhz * 1e6).round() as u64);
        // The Cabrillo dial rides in its own tag. A journal written before that tag existed
        // put the DIAL in `FREQ`, so falling back to it restores exactly what that row meant
        // — never a dial reconstructed from an on-air value it does not carry.
        let freq_khz = f
            .get("APP_NEXUS_DIALKHZ")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|khz| *khz > 0)
            .or_else(|| mhz_of("FREQ").map(|mhz| (mhz * 1000.0).round() as u32))
            .unwrap_or(0);
        // ⭐ THE BIRD, restored from the pair the writer can only emit together — so a
        // mid-event restart does not silently strip the satellite identity off a pass
        // contact on its way to the merged lifetime log and LoTW. A row with no
        // `SAT_NAME` is terrestrial as far as this build can tell, and `PROP_MODE`
        // without one is not a pair and restores nothing. The leg's own band and
        // frequency are the ones already read back above, because that IS what they
        // were written from.
        // The leg's frequency, now that the journaled one has been read. It is the
        // downlink this row was written from, which is what `adif` wrote as `FREQ`.
        let sat = sat.map(|s| SatLeg {
            down_hz: on_air_hz,
            ..s
        });
        let row = LoggedQso {
            call: call.clone(),
            rx,
            tx,
            role,
            entity: placed.map(|p| p.entity.to_string()),
            continent: placed.map(|p| p.continent.to_string()),
            prefix: crate::contest::wpx_prefix(call),
            band,
            freq_khz,
            on_air_hz,
            freq_rx_hz,
            mode: mode.to_string(),
            submode,
            slot: 0,
            when_unix,
            seq,
            dupe,
            sat,
            // Who was at the key, from the journal's own `OPERATOR` — a row journaled before
            // the tag existed names nobody, which is what it always meant.
            operator: f
                .get("OPERATOR")
                .map(|o| o.trim().to_ascii_uppercase())
                .unwrap_or_default(),
            // A journaled row went out under the log's own call: only a club's rebuild sets
            // another, and that log is never journaled.
            station_call: String::new(),
            // The call it was logged on, from the journal's own `APP_NEXUS_ONAIR`: a row
            // journaled before the tag existed carries none.
            on_air_call: f
                .get("APP_NEXUS_ONAIR")
                .map(|c| c.trim().to_ascii_uppercase())
                .unwrap_or_default(),
            // The radio and the power, from the journal's own `MY_RIG` and `TX_PWR`: a row
            // journaled before they were recorded carries neither.
            rig: f
                .get("MY_RIG")
                .map(|r| r.trim().to_string())
                .unwrap_or_default(),
            tx_power: f
                .get("TX_PWR")
                .map(|p| p.trim().to_string())
                .unwrap_or_default(),
            // The grid, from the journal's `APP_NEXUS_MYGRID`: none on an older row.
            grid: f
                .get("APP_NEXUS_MYGRID")
                .map(|g| g.trim().to_ascii_uppercase())
                .unwrap_or_default(),
        };
        match removed_unix {
            Some(at) => self.keep_removed(row, at),
            None => self.qsos.push(row),
        }
        RowFate::Restored
    }

    /// A removed row read back out of the journal joins the entry it was removed with: the
    /// journal writes one removal's rows together (a county line: one call, one time, one
    /// mark), so a row continuing the last entry is that entry's next row.
    fn keep_removed(&mut self, row: LoggedQso, removed_unix: u64) {
        match self.removed.last_mut() {
            Some(last)
                if last.removed_unix == removed_unix
                    && last
                        .rows
                        .last()
                        .is_some_and(|q| q.call == row.call && q.when_unix == row.when_unix) =>
            {
                last.rows.push(row)
            }
            _ => self.removed.push(RemovedQso {
                rows: vec![row],
                removed_unix,
            }),
        }
    }

    /// Export the log as a Cabrillo entry — headers (§6.1) then one QSO line per
    /// contact (§6.2) — for the given band frequency (kHz).
    ///
    /// `Err` carries the reason this log is not ONE submittable entry, in words meant
    /// for the export dialog. Today there is exactly one such reason and no shipped
    /// contest can reach it: a mode-split contest whose rows span both modes submits
    /// as two entries, and a file cannot be both. It is fallible rather than
    /// best-effort because the alternative is writing a file that looks right and is
    /// scored under the wrong id.
    pub fn cabrillo(&self, freq_khz: u32) -> Result<String, String> {
        self.cabrillo_with(freq_khz, &crate::contest::CabrilloEntrant::default())
    }

    /// The header block [`cabrillo_with`](Self::cabrillo_with) writes for `entrant` — the one
    /// place it is built, so the export and [`cabrillo_missing`](Self::cabrillo_missing) cannot
    /// come to disagree about which lines a file carries. `Err` is the export's own refusal.
    fn cabrillo_header_block(
        &self,
        entrant: &crate::contest::CabrilloEntrant,
    ) -> Result<crate::contest::CabrilloHeaders, String> {
        let spec = self.session.exchange;
        let rs = self.ruleset();
        // An optional header is written only when the sponsor's template lists it (the
        // rules file's `cabrillo.headers`); a value the ruleset does not ask for is "".
        let declared = |tag: &str, value: String| {
            if rs.cabrillo_headers.contains(&tag) {
                value
            } else {
                String::new()
            }
        };
        // …and the same rule for the headers that state the entry, which a ruleset lists
        // under `cabrillo.entry_headers` (see `FdRuleset::cabrillo_entry_headers` for why
        // that is a list of its own).
        let declared_entry = |tag: &str, value: String| {
            if rs.cabrillo_entry_headers.contains(&tag) {
                value
            } else {
                String::new()
            }
        };
        // ⭐ The `CONTEST` token, resolved against the mode classes this log actually
        // holds. A mode-split contest submits a separate entry per mode, so one file
        // holding both is refused BY NAME rather than filed under whichever id came
        // first (§6.2). Nothing this build ships is split, so this is always the
        // session's own id — read from the SESSION, which is the only place that can
        // name a contest `FdEvent` has no arm for (see [`Self::ruleset`]).
        // ⭐ FIRING SITE 2 of 3, at the point of use (§6.3): a log whose rows send two
        // different values for a slot the sponsor says is constant is NOT one
        // submittable entry — SS-Rules v2.1 §4.4.2, *"The same Check must be sent
        // throughout the contest"* — which is the same class of refusal this function
        // already makes for a mode-split log holding both modes. It refuses to export
        // SILENTLY; the message names both values and the row counts so the export
        // dialog can say what to fix. (Logging a contact is never blocked: that is
        // site 1, which warns.)
        if let Some(m) = self.constant_sent_scan() {
            return Err(m.message());
        }
        let mut classes: Vec<&str> = self.qsos.iter().map(|q| q.mode.as_str()).collect();
        classes.sort_unstable();
        classes.dedup();
        // ⭐ A SPONSOR WHOSE `LOCATION` IS WHAT THE ENTRY SENDS (NYQP: the county for a New
        // York entry, the state, province or `DX` for everyone else). *"For mobile and
        // portable stations in New York, use the NYQP-approved three-letter county
        // abbreviation of any county activated during the contest"* — so the value is read
        // off the first ROW that sent one, a county a contact was actually made from, and
        // not off the county being composed now, which a mobile that has moved on and not
        // yet worked anybody has not activated. An empty log has activated nothing, and
        // heads the county it is sending from.
        let slot = rs.cabrillo_location_slot;
        let sent_location = (!slot.is_empty())
            .then(|| {
                self.qsos
                    .iter()
                    .map(|q| q.sent(slot).trim())
                    .find(|v| !v.is_empty())
                    .unwrap_or_else(|| self.session.field(slot).trim())
                    .to_string()
            })
            .filter(|v| !v.is_empty());
        // ⭐ A MULTI-OPERATOR ENTRY'S CLASS (`CATEGORY-TRANSMITTER`): written only for a MULTI-OP
        // entry — Cabrillo's "The category-transmitter is required for multi-operator entries" —
        // and only when it is one of this sponsor's own, so a class left over from another
        // contest, or from a weekend as a multi-operator, claims nothing.
        let transmitter =
            if self.session.entry_category == crate::contest::OperatorCategory::MultiOp {
                rs.cabrillo_transmitter_categories
                    .iter()
                    .find(|c| c.eq_ignore_ascii_case(entrant.transmitter.trim()))
                    .map_or_else(String::new, |c| c.to_string())
            } else {
                String::new()
            };
        // The band the ENTRY declares, when it is one of the sponsor's (`ALL` or one band).
        let declared_band = rs
            .cabrillo_band_categories
            .iter()
            .find(|b| b.eq_ignore_ascii_case(entrant.band.trim()));
        // ⭐ A SINGLE-BAND ENTRY WHOSE LOG HOLDS ANOTHER BAND. CQ WW and WPX (X.2, and WPX's own
        // log instructions): *"Single band entrants are required to include all contacts made
        // during the contest period, even if on other bands. Only contacts made on the band
        // specified in the Cabrillo header will be considered for scoring purposes."* The score
        // here counts every band, so for such an entry it is more than the sponsor credits.
        let off_declared_band = declared_band.is_some_and(|b| {
            !b.eq_ignore_ascii_case("ALL")
                && self.qsos.iter().any(|q| {
                    !cabrillo_category_band(std::slice::from_ref(q)).eq_ignore_ascii_case(b)
                })
        });
        let headers = crate::contest::CabrilloHeaders {
            // ⭐ …then translated out of ADIF's namespace into Cabrillo's. The id
            // above is an ADIF `CONTEST_ID` enumeration value, which is NOT the
            // Cabrillo `CONTEST:` token for every contest — ARRL Field Day is
            // `ARRL-FIELD-DAY` to ADIF and `ARRL-FD` to Cabrillo. This is the last
            // step before the header and the only place the two registries meet;
            // the ADIF export above reads the id directly and must keep doing so.
            //
            // ⚠️ The id it translates comes from the SESSION, not from `self.event`:
            // `FdEvent` has no arm for a QSO party, so reading it here would hand
            // every party's log to the Cabrillo mapper under Field Day's id and get
            // `ARRL-FD` back.
            contest: crate::contest::cabrillo_contest_token(&crate::contest::resolve_contest_id(
                &self.session.contest_id,
                &self.session.contest_id_by_mode,
                &classes,
            )?)
            .to_string(),
            callsign: self.mycall.clone(),
            // ⭐ The declaration, not a literal. `CATEGORY-OPERATOR: MULTI-OP` was
            // hardcoded here, so every solo entry submitted a claim that more than
            // one operator was at the station.
            category_operator: self.session.entry_category,
            // LOCATION is a per-ENTRY value, so it reads the session's declared
            // location and not a row: Cabrillo puts it in a header, once, for exactly
            // that reason. The per-contact truth is on the QSO lines below. (A sponsor
            // that ties LOCATION to what the entry SENDS is the exception, above.)
            //
            // A `dx`-role entry is `DX` whatever its state field holds — §2.3's role word
            // for it, and what CQ WW RTTY X.3 asks for: "other stations indicate 'DX'". A
            // sponsor whose LOCATION list spells a place differently from its exchange
            // (CQ WW RTTY's `PEI` is `PE` there) maps it through `cabrillo.location`.
            location: sent_location.unwrap_or_else(|| {
                if self.session.role().id == "dx" {
                    "DX".to_string()
                } else {
                    let state = self.session.my_location.state.as_str();
                    rs.cabrillo_location
                        .iter()
                        .find(|(from, _)| from.eq_ignore_ascii_case(state))
                        .map_or(state, |(_, to)| to)
                        .to_string()
                }
            }),
            created_by: "Nexus".to_string(),
            // ⭐ The sponsor's own X- headers first, each the value the entry SENT in its
            // slot (Winter Field Day's `X-EXCHANGE: 3O`, its class and category), read off
            // the first row like a sent LOCATION and off the session for an empty log; a
            // slot with nothing in it writes no line. Then which rules data scored this log
            // (X- headers are Cabrillo-legal and ignored by robots) — a fetched rules file
            // with different parameters is visible on the artifact an operator submits.
            x_headers: rs
                .cabrillo_x_headers
                .iter()
                .filter_map(|(name, slot)| {
                    let sent = self
                        .qsos
                        .iter()
                        .map(|q| q.sent(slot).trim())
                        .find(|v| !v.is_empty())
                        .unwrap_or_else(|| self.session.field(slot).trim());
                    (!sent.is_empty()).then(|| (name.to_string(), sent.to_string()))
                })
                .chain(std::iter::once((
                    "X-NEXUS-RULES-YEAR".to_string(),
                    self.ruleset().rules_year.to_string(),
                )))
                .collect(),
            // ⭐ The optional headers. The two declarations were read when the session
            // started, like CATEGORY-OPERATOR; the band and mode are what the log holds;
            // NAME and EMAIL are the entrant's settings, read by the caller at export.
            category_assisted: declared(
                "CATEGORY-ASSISTED",
                self.session.category_assisted.clone(),
            ),
            // A sponsor that names the powers it takes (Winter Field Day: QRP or LOW) gets
            // the declared power only when it is one of them; HIGH there claims nothing.
            category_power: if rs.cabrillo_power_categories.is_empty() {
                declared("CATEGORY-POWER", self.session.category_power.clone())
            } else {
                rs.cabrillo_power_categories
                    .iter()
                    .find(|c| c.eq_ignore_ascii_case(self.session.category_power.trim()))
                    .map_or_else(String::new, |c| c.to_string())
            },
            // ⭐ The band the ENTRY declares, where the sponsor lists the bands it may (CQ: "If
            // you are entering an all band category, use ALL, otherwise select one of the
            // individual bands shown"); a band picked for another contest claims nothing. A
            // sponsor whose template takes the band off the log (CQ WW RTTY, X.2) still gets that
            // for an entry that declares none.
            category_band: declared_band.map_or_else(
                || declared("CATEGORY-BAND", cabrillo_category_band(&self.qsos)),
                |b| b.to_string(),
            ),
            // The contest's own mode where the ruleset names it (every CQ WW CW entry is a CW
            // entry), and otherwise the log's, where the sponsor's template takes that.
            category_mode: if rs.cabrillo_mode_category.is_empty() {
                declared("CATEGORY-MODE", cabrillo_category_mode(&self.qsos))
            } else {
                rs.cabrillo_mode_category.to_string()
            },
            // The claimed score is the one the screen shows, and only when that score is
            // the whole score: no power tier or ticked bonus after it, and no note saying
            // it leaves something out. An objectives contest WITH its menu (Winter Field
            // Day) is whole too: the sponsor asks for "your calculated total score
            // including multipliers", and the objectives ticked are the entrant's own. A
            // single-band entry with a contact on another band claims nothing (above).
            claimed_score: (rs.cabrillo_headers.contains(&"CLAIMED-SCORE")
                && !self.qsos.is_empty()
                && !off_declared_band
                && (rs.scoring.post.is_empty() || !rs.objective_menu.is_empty())
                && rs.score_note_key.is_empty())
            .then(|| {
                // ⭐ …plus the bonus stations this LOG earned, which the sponsor adds
                // "to the final score" (ILQP's two club calls). They are derived from
                // the rows rather than claimed on a menu, so a file that left them out
                // would claim 100 or 200 points less than the club will credit. The
                // ticked-bonus menu is the other thing entirely, and a ruleset carrying
                // one takes the `post` arm above and writes no claimed score at all.
                let (qso_points, _, _, scored) = rs.scoring.score(self.score_rows(), 1);
                rs.claimed_total(
                    qso_points,
                    scored,
                    self.bonus_station_points(),
                    &entrant.objectives,
                )
            }),
            email: declared("EMAIL", entrant.email.trim().to_string()),
            name: declared("NAME", entrant.name.trim().to_string()),
            // ⭐ THE SPONSOR'S OWN HEADER, for the entrant it is about. ILQP's sample
            // log heads an Illinois entry `IL-COUNTY: ADAMS` — the NAME of the county,
            // beside QSO lines carrying its code — and an entry from anywhere else has
            // no Illinois county to name. The value is read back off the exchange this
            // session is COMPOSING (the first slot whose copied value matched a domain,
            // which is the in-state role's own location slot), so the header and the
            // QSO lines cannot disagree about where the entry ran from.
            il_county: declared(
                "IL-COUNTY",
                (self.session.role().id == "in_state")
                    .then(|| {
                        self.session
                            .my_exchange
                            .iter()
                            .find_map(|v| spec.name_of(v))
                    })
                    .flatten()
                    .unwrap_or_default()
                    .to_string(),
            ),
            // ⭐ THE ENTRY'S OWN LINES, for a sponsor whose processing software reads them
            // (the Illinois QSO Party: *"make sure entry class, personal information, call
            // sign, station location, and club affiliation are correctly shown"*). The club
            // and the class are the entrant's settings; a class is written only when it is
            // one of THIS sponsor's, so a pick left over from another contest claims nothing.
            club: declared_entry("CLUB", entrant.club.trim().to_string()),
            entry_class: declared_entry(
                "ENTRY-CLASS",
                rs.entry_classes
                    .iter()
                    .find(|c| c.eq_ignore_ascii_case(entrant.entry_class.trim()))
                    .map_or_else(String::new, |c| c.to_string()),
            ),
            // …and for a multi-operator entry whose class this sponsor takes, because every
            // sponsor that takes one asks such an entry for its operators (CQ WW: "If you select
            // a multi-operator category and do not enter operator callsigns in this field, the
            // robot will reject your log").
            operators: if rs.cabrillo_entry_headers.contains(&"OPERATORS")
                || !transmitter.is_empty()
            {
                cabrillo_operators(&self.qsos, &entrant.operators)
            } else {
                String::new()
            },
            // The sponsor's QRP certification follows the power the entry DECLARED, read
            // when the session started like CATEGORY-POWER; anything else claims nothing.
            qrp_competition: declared_entry(
                "QRP-COMPETITION",
                if self.session.category_power.eq_ignore_ascii_case("QRP") {
                    "YES".to_string()
                } else {
                    String::new()
                },
            ),
            // ⭐ THE DECLARATIONS A SPONSOR'S TEMPLATE NAMES, from the entrant's own settings and
            // only where this ruleset lists them: an overlay only when it is one this sponsor
            // scores, the address in the headers its template carries, the soapbox only where
            // it has one. A pick made for another contest claims nothing here.
            category_transmitter: transmitter,
            category_overlay: rs
                .cabrillo_overlay_categories
                .iter()
                .find(|c| c.eq_ignore_ascii_case(entrant.overlay.trim()))
                .map_or_else(String::new, |c| c.to_string()),
            address: crate::contest::address_lines(&entrant.address, rs.cabrillo_address_headers),
            soapbox: if rs.cabrillo_soapbox {
                crate::contest::soapbox_lines(&entrant.soapbox)
            } else {
                Vec::new()
            },
        };
        Ok(headers)
    }

    /// [`cabrillo`](Self::cabrillo) with the entrant's `NAME` and `EMAIL`, which come from
    /// the operator's own settings at export time (the caller reads them; this crate holds
    /// none). Each is written only where the ruleset lists it and it holds a value.
    ///
    /// ⭐ **Field Day's file is exactly what it has always been.** The optional headers are
    /// written only for a ruleset whose `cabrillo.headers` names them (neither Field Day
    /// event does), and the dial frequency on a QSO line is never used for Field Day: both
    /// files are pinned byte for byte by the §8(a) goldens.
    pub fn cabrillo_with(
        &self,
        freq_khz: u32,
        entrant: &crate::contest::CabrilloEntrant,
    ) -> Result<String, String> {
        let spec = self.session.exchange;
        let rs = self.ruleset();
        let field_day = spec.name == "fieldday";
        let mut s = self.cabrillo_header_block(entrant)?.render();
        for q in self.submitted() {
            // QSO: freq mo date time mycall myexch call exch — ARRL requires a
            // REAL `yyyy-mm-dd hhmm`; the old `----------` placeholder failed
            // submission. Legacy rows without a stamp keep the placeholder so
            // we never invent a time.
            let (date, time) = if q.when_unix > 0 {
                cabrillo_datetime(q.when_unix)
            } else {
                ("----------".to_string(), "----".to_string()) // HHMM is 4 chars
            };
            // Mode token per Cabrillo 3.0: CW, PH phone, RY for RTTY rows
            // (WFD prefers it), DG for other/unrecorded digital — a legal
            // fallback either event.
            let mo = match q.mode.as_str() {
                "CW" => "CW",
                "PH" => "PH",
                _ if q.submode == "RTTY" => "RY",
                _ => "DG",
            };
            // Per-QSO frequency from ITS band. The caller's dial is a fallback ONLY for a
            // row with no band recorded at all — never for a band we simply failed to map,
            // because that is how a 23 cm club contact came to export as 20 m: the club
            // exporter passes a hardcoded 14 MHz and every unmapped band took it. A band we
            // do not recognise now rides through as itself; a wrong-looking token in one
            // field beats a confident lie about which band a contact was made on, and ARRL
            // Field Day requires the worked list sorted BY BAND.
            let mapped = band_to_cabrillo_freq(&q.band);
            let raw = q.band.trim();
            let freq: String = match (mapped, raw.is_empty()) {
                // ⭐ The dial the contact was logged on, when the row knows it and it lies
                // inside the row's own band (CQ WW RTTY X.1 asks award contenders for
                // "accurate frequencies for all contacts"). Never for Field Day (see
                // `cabrillo_with`), and never a dial outside the band, which would be a
                // confident lie about where the contact was made.
                _ if !field_day && khz_in_band(q.freq_khz, &q.band) => q.freq_khz.to_string(),
                (Some(f), _) => f.to_string(),
                (None, false) => raw.to_string(),
                (None, true) => freq_khz.to_string(),
            };
            // ⭐ THE EXCHANGE COLUMNS COME FROM THIS ROW, never from the log or the
            // session. `self.myexch.class`/`.section` sat here until batch 3 and were
            // written on EVERY line, so an operator who changed county re-labelled
            // every contact made before the change — silently, in the file they submit.
            //
            // ⭐ **The callsign columns are STRUCTURAL — they sit outside the
            // exchange — and a side whose exchange ALSO declares a `Call` slot does not
            // repeat it.** A template of exchange slots alone loses both callsigns on
            // every contest, and an unsubmittable line is the failure mode; Sweepstakes
            // is the one contest whose on-air exchange carries a callsign too, so
            // without the exception its line would show four. ARRL's own published SS
            // template puts the callsign in the structural column (`E= Your call`,
            // `J= The call of the station you worked`) with serial/precedence/check/
            // section after it, so the slot's Cabrillo home IS that column — see
            // [`contest::cabrillo::side_declares_call`] for the template and the legend.
            // Read off the slot list, so there is nothing a ruleset can declare
            // inconsistently.
            let role = crate::contest::role_for(q, spec);
            let mut cols: Vec<String> = vec![freq, mo.to_string(), date, time];
            let is_call = |k: &str| crate::contest::is_call_slot(spec, k);
            // ⭐ A SPONSOR'S OWN TEMPLATE, when the ruleset declares one: the same fixed
            // columns on both sides of every line, whichever role sent the row, padded and
            // with a placeholder for a blank (CQ WW RTTY: a two-digit zone, and `DX` where a
            // station has no QTH — on its own side too, although a DX station sends none
            // on the air). Without one, the structural line, unchanged.
            let template = rs.cabrillo_columns;
            // The call SENT: the log's own, or the one this row went out under — a GOTA
            // station's contact in an ARRL Field Day club's file (`LoggedQso::station_call`).
            cols.push(if q.station_call.is_empty() {
                self.mycall.clone()
            } else {
                q.station_call.clone()
            });
            if template.is_empty() {
                cols.extend(
                    crate::contest::sent_exchange(q, spec)
                        .into_iter()
                        .filter(|v| !is_call(v.key))
                        .map(|v| v.raw)
                        .filter(|r| !r.is_empty()),
                );
            } else {
                cols.extend(
                    template
                        .iter()
                        .map(|c| c.cell(q.sent(c.key)))
                        .filter(|c| !c.is_empty()),
                );
            }
            cols.push(q.call.clone());
            if template.is_empty() {
                cols.extend(
                    role.receives
                        .iter()
                        .filter(|k| !is_call(k))
                        .map(|k| q.rcvd(k))
                        .filter(|v| !v.is_empty())
                        .map(str::to_string),
                );
            } else {
                cols.extend(
                    template
                        .iter()
                        .map(|c| c.cell(q.rcvd(c.key)))
                        .filter(|c| !c.is_empty()),
                );
            }
            // The trailing transmitter-id column, present only where the sponsor's own
            // template has one — never for either Field Day event.
            if let Some(t) = self.session.transmitter_id {
                cols.push(t.to_string());
            }
            // Every value on the line came from a row; none of them can end it.
            s.push_str(&format!(
                "QSO: {}\n",
                crate::contest::cabrillo::one_line(&cols.join(" "))
            ));
        }
        s.push_str("END-OF-LOG:\n");
        Ok(s)
    }

    /// ⭐ **The header lines this contest's sponsor requires that this log's Cabrillo file would
    /// leave out**, in the ruleset's order — its
    /// [`cabrillo_required`](crate::fd_rules::FdRuleset::cabrillo_required), read against the
    /// header [`cabrillo_with`](Self::cabrillo_with) writes for `entrant`, so the Contest screen
    /// can name each one before the export. Never a refusal: the file is the entrant's to send.
    ///
    /// `CATEGORY-TRANSMITTER` and `OPERATORS` are a multi-operator entry's (Cabrillo: *"The
    /// category-transmitter is required for multi-operator entries"*; CQ: *"If you select a
    /// multi-operator category and do not enter operator callsigns in this field, the robot will
    /// reject your log"*), so a single operator's file is never missing them; and a `CHECKLOG` is
    /// placed in no category, so nothing is missing from it. A log the export refuses names
    /// nothing here: the refusal is its own sentence.
    pub fn cabrillo_missing(&self, entrant: &crate::contest::CabrilloEntrant) -> Vec<&'static str> {
        use crate::contest::OperatorCategory;
        const MULTI_OPERATOR_LINES: [&str; 2] = ["CATEGORY-TRANSMITTER", "OPERATORS"];
        let rs = self.ruleset();
        let category = self.session.entry_category;
        if rs.cabrillo_required.is_empty() || category == OperatorCategory::Checklog {
            return Vec::new();
        }
        let Ok(header) = self.cabrillo_header_block(entrant) else {
            return Vec::new();
        };
        let header = header.render();
        rs.cabrillo_required
            .iter()
            .copied()
            .filter(|tag| {
                category == OperatorCategory::MultiOp || !MULTI_OPERATOR_LINES.contains(tag)
            })
            .filter(|tag| {
                !header
                    .lines()
                    .any(|l| l.split_once(": ").is_some_and(|(t, _)| t == *tag))
            })
            .collect()
    }
}

/// The received slots that have a standard ADIF column, as `(slot id, tag)`, in the
/// role's receive order.
///
/// ⭐ **ONE list, read by the ADIF writer and by the restore.** The write direction and
/// the read direction being the same function is what makes "the standard columns are
/// enough for this row" and "no private carrier was written" the same fact rather than
/// two claims that can drift apart. A slot with no `rcvd` tag has no standard column
/// either way round, and rides [`carrier`](crate::contest::carrier) instead.
fn standard_rcvd_slots(
    spec: &'static crate::contest::ExchangeSpec,
    role: &'static crate::contest::RoleSpec,
) -> impl Iterator<Item = (&'static str, &'static str)> {
    role.receives
        .iter()
        .filter_map(move |key| Some((*key, spec.field(key)?.adif.rcvd?)))
}

/// The received exchange as the standard ADIF columns carry it — the restore FALLBACK
/// for a journal with no `APP_NEXUS_EX`, and the writer's test for whether that tag is
/// needed at all. `get` resolves an ADIF tag to its value.
fn rx_from_standard(
    spec: &'static crate::contest::ExchangeSpec,
    role: &'static crate::contest::RoleSpec,
    get: impl Fn(&str) -> Option<String>,
) -> Vec<crate::contest::FieldValue> {
    standard_rcvd_slots(spec, role)
        .filter_map(|(key, tag)| spec.value(key, &get(tag)?))
        .collect()
}

/// The serial a row was SENT under — the number
/// [`ContestSession::compose_for`](crate::contest::ContestSession::compose_for) issued
/// onto it, read back off the row's own sent exchange.
///
/// ⭐ **The slot is found by KIND, not by name.** Sweepstakes calls it `NR`, CQ WPX and
/// the California QSO Party call it `SERIAL`, and a rules file is free to call it
/// something else again — a name list here would silently stop finding the slot on the
/// sixth contest that ships, which is precisely the failure the restore must not have.
///
/// `None` when the exchange declares no serial at all (both Field Day events, the QSO
/// parties that send a county), when the row carries no value for the slot, or when
/// that value will not parse — a journal is a file on disk and the slot can hold
/// anything. **Zero is `None` too**: `"0"` is the template placeholder
/// (`session::sent_value`'s `"serial"` arm), so a row logged without a compose carries
/// it and numbers nothing.
fn issued_serial(
    spec: &'static crate::contest::ExchangeSpec,
    tx: &[crate::contest::FieldValue],
) -> Option<u32> {
    tx.iter()
        .find(|v| {
            matches!(
                spec.field(v.key).map(|f| f.kind),
                Some(crate::contest::FieldKind::Serial { .. })
            )
        })
        .and_then(|v| v.raw.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A band-representative frequency in kHz for the Cabrillo QSO line. Field Day
/// isn't scored by frequency, and we only store the band per contact — so a
/// multi-band log must stamp each row with ITS band, not the single dial the
/// export happened to be sitting on. `None` for an unrecognized band → caller's
/// fallback.
fn band_to_cabrillo_freq(band: &str) -> Option<&'static str> {
    Some(match band.trim().to_ascii_lowercase().as_str() {
        // HF: a representative frequency in kHz.
        "160m" => "1800",
        "80m" => "3500",
        "60m" => "5330",
        "40m" => "7000",
        "30m" => "10100",
        "20m" => "14000",
        "17m" => "18068",
        "15m" => "21000",
        "12m" => "24890",
        "10m" => "28000",
        // ⚠️ 50 MHz AND UP IS A BAND TOKEN, NOT KILOHERTZ. The Cabrillo QSO-data spec:
        // "freq is frequency or band: 1800 or actual frequency in kHz […] 50 / 70 / 144 /
        // 222 / 432 / 902 / 1.2G / 2.3G". Winter Field Day repeats the rule and machine-parses
        // the QSO lines out of whatever is uploaded, so this is the half that actually gets
        // read. We used to emit 50000 / 144000 / 222000, and 70 cm as 420000 — wrong form, and
        // for 70 cm not even a sensible frequency; the token is 432.
        "6m" => "50",
        "4m" => "70",
        "2m" => "144",
        "1.25m" => "222",
        "70cm" => "432",
        "33cm" => "902",
        "23cm" => "1.2G",
        "13cm" => "2.3G",
        "9cm" => "3.4G",
        "6cm" => "5.7G",
        "3cm" => "10G",
        _ => return None,
    })
}

/// Is `khz` inside the amateur allocation `band` names? HF only: at 50 MHz and up the
/// Cabrillo field is a band TOKEN (`50`, `144`), never kilohertz, so no dial is written
/// there whatever it says.
fn khz_in_band(khz: u32, band: &str) -> bool {
    let (lo, hi) = match band.trim().to_ascii_lowercase().as_str() {
        "160m" => (1_800, 2_000),
        "80m" => (3_500, 4_000),
        "60m" => (5_250, 5_450),
        "40m" => (7_000, 7_300),
        "30m" => (10_100, 10_150),
        "20m" => (14_000, 14_350),
        "17m" => (18_068, 18_168),
        "15m" => (21_000, 21_450),
        "12m" => (24_890, 24_990),
        "10m" => (28_000, 29_700),
        _ => return false,
    };
    (lo..=hi).contains(&khz)
}

/// Cabrillo `CATEGORY-BAND` for a log: `ALL` when it holds more than one band, the one
/// band's token when it holds one (`20M`), and `""` — no header — for an empty log or a
/// band Cabrillo has no token for. It is only ever written for a ruleset that lists the
/// header, and CQ WW RTTY classifies logs exactly this way (X.2: "Logs with contacts only
/// on one band will be classified as single band entries").
/// Cabrillo `OPERATORS`: everyone the rows say was at the key, in the order they first
/// logged, then whoever the entrant typed (callsigns split on spaces or commas). Uppercase,
/// each once, space-separated as Cabrillo 3.0 lists them. `""` when nobody is named, which
/// writes no line: a single-op station's call is already the entry's `CALLSIGN`.
fn cabrillo_operators(qsos: &[LoggedQso], typed: &str) -> String {
    let mut ops: Vec<String> = Vec::new();
    let named = qsos
        .iter()
        .map(|q| q.operator.as_str())
        .chain(typed.split(|c: char| c == ',' || c.is_whitespace()));
    for op in named {
        let op = op.trim().to_ascii_uppercase();
        if !op.is_empty() && !ops.contains(&op) {
            ops.push(op);
        }
    }
    ops.join(" ")
}

fn cabrillo_category_band(qsos: &[LoggedQso]) -> String {
    let mut bands = qsos.iter().map(|q| q.band.trim().to_ascii_lowercase());
    let Some(first) = bands.next() else {
        return String::new();
    };
    if bands.any(|b| b != first) {
        return "ALL".to_string();
    }
    match first.as_str() {
        "160m" => "160M",
        "80m" => "80M",
        "40m" => "40M",
        "20m" => "20M",
        "15m" => "15M",
        "10m" => "10M",
        "6m" => "6M",
        "4m" => "4M",
        "2m" => "2M",
        "1.25m" => "222",
        "70cm" => "432",
        "33cm" => "902",
        "23cm" => "1.2G",
        _ => "",
    }
    .to_string()
}

/// Cabrillo `CATEGORY-MODE` for a log, from the modes its rows were worked in: one mode
/// names itself (`CW`, `SSB`, `FM`, `RTTY`, `DIGI` for any other digital mode), more than
/// one is `MIXED`, and an empty log writes no header.
fn cabrillo_category_mode(qsos: &[LoggedQso]) -> String {
    let mut kinds = qsos.iter().map(|q| match q.mode.as_str() {
        "CW" => "CW",
        // ⭐ FM IS A CLASS, NOT A SIDE, and Cabrillo has a token for it. `PH` is the
        // SCORING class — it covers SSB, AM and FM alike — so it cannot name the entry's
        // mode by itself, and an FM-only log declared `CATEGORY-MODE: SSB`. The V3
        // enumeration is `CW / DIGI / FM / RTTY / SSB / MIXED`: there is no `AM` token,
        // so AM rides as the phone one, which is the closest legal claim.
        "PH" => {
            if q.submode == "FM" {
                "FM"
            } else {
                "SSB"
            }
        }
        _ if q.submode == "RTTY" => "RTTY",
        _ => "DIGI",
    });
    let Some(first) = kinds.next() else {
        return String::new();
    };
    if kinds.all(|k| k == first) {
        first.to_string()
    } else {
        "MIXED".to_string()
    }
}

/// Unix seconds → ("yyyy-mm-dd", "hhmm") in UTC for two Cabrillo fields.
fn cabrillo_datetime(unix: u64) -> (String, String) {
    let (y, mo, d, h, mi, _s) = civil_from_unix(unix);
    (format!("{y:04}-{mo:02}-{d:02}"), format!("{h:02}{mi:02}"))
}

/// Unix seconds → ("yyyymmdd", "hhmmss") in UTC for ADIF QSO_DATE / TIME_ON.
fn adif_datetime(unix: u64) -> (String, String) {
    let (y, mo, d, h, mi, s) = civil_from_unix(unix);
    (
        format!("{y:04}{mo:02}{d:02}"),
        format!("{h:02}{mi:02}{s:02}"),
    )
}

/// Unix seconds → civil UTC (y, mo, d, h, mi, s) (Howard Hinnant's
/// civil-from-days; no external date crate needed for a few export fields).
fn civil_from_unix(unix: u64) -> (i64, u32, u32, u32, u32, u32) {
    let secs_of_day = unix % 86_400;
    let days = (unix / 86_400) as i64;
    let (h, mi, s) = (
        (secs_of_day / 3600) as u32,
        ((secs_of_day % 3600) / 60) as u32,
        (secs_of_day % 60) as u32,
    );
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi, s)
}

/// Civil UTC → Unix seconds — the inverse of [`civil_from_unix`], for restoring
/// journal timestamps (mirrors `logbook.rs::unix_from_ymdhms`).
fn unix_from_ymdhms(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> u64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + (h as i64) * 3600 + (mi as i64) * 60 + s as i64;
    secs.max(0) as u64
}

fn adif_field(name: &str, value: &str) -> String {
    // ADIF's String is printable characters, and a reader that goes by lines would read a
    // record a value broke as two: no value can end its record's line.
    let value = crate::contest::cabrillo::one_line(value);
    format!("<{}:{}>{} ", name, value.len(), value)
}

// ---- Auto-sequencer -------------------------------------------------------

/// Which messages the FT8/FT4 sequencer puts on the air, named after the WSJT-X 3.0.2 Special
/// Operating Activity a station in the same contest runs (`Configuration.hpp`,
/// `SpecialOperatingActivity`) — that program is what the station on the other end of the
/// contact is sequencing with.
///
/// ⚠️ **Only FT8 and FT4 follow the contest.** FT1, DX1 and FT2 have no WSJT-X contest
/// messages to follow, and keep [`FieldDay`](Self::FieldDay)'s; so does every contest whose
/// rules ban the tier's mode. Neither is changed by anything below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FtMessages {
    /// ARRL Field Day (`FIELD_DAY`): `CQ FD`, then the class and section, which the 77-bit
    /// Field Day message carries (i3=0, n3=3).
    #[default]
    FieldDay,
    /// The ARRL VHF contests (`NA_VHF`, "NA VHF Contest"): `CQ TEST`, then the
    /// four-character grid, rogered as `R <grid>`, all standard (i3=1) messages; the runner
    /// closes with 73.
    NaVhf,
    /// A contest WSJT-X has no activity for: the standard messages every FT8/FT4 contact
    /// outside a contest uses — a plain CQ, the grid, a report, `R` and a report, RR73, 73.
    Standard,
}

impl FtMessages {
    /// The WSJT-X activity a contest's own stations run, by the rules file's event id (blank is
    /// a fresh install's contest, ARRL Field Day).
    ///
    /// By id, because which activity a contest's stations run is a fact about the contest that
    /// its exchange does not state: the WW Digi contest exchanges grids too, and calls `CQ WW`.
    ///
    /// ⚠️ **Winter Field Day has none**, though its exchange has Field Day's shape: WSJT-X's
    /// Field Day exchange takes the class letters A to F only (`Configuration.cpp`,
    /// `field_day_exchange_re`), and Winter Field Day's categories are H, I, O and M.
    pub fn of_contest(event: &str) -> Self {
        match event {
            "" | "arrlfd" => Self::FieldDay,
            "arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep" => Self::NaVhf,
            _ => Self::Standard,
        }
    }

    /// The messages `ruleset`'s sequencer sends on a tier whose ADIF mode is `mode` (`"FT8"`,
    /// `"FT4"`, `"FT2"`, `"TempoFast"`, …): its contest's own ([`of_contest`](Self::of_contest))
    /// on FT8 and FT4, and Field Day's, unchanged, on every other tier, wherever the rules ban
    /// the mode, and in Winter Field Day.
    pub fn for_contest(ruleset: &crate::fd_rules::FdRuleset, mode: &str) -> Self {
        if !Self::follows_contest(ruleset, mode) {
            return Self::FieldDay;
        }
        match ruleset.event {
            // Field Day's exchange, as it has always sent: WSJT-X has no activity for it, and a
            // rules file that stopped banning the mode would not change what goes on the air.
            "wfd" => Self::FieldDay,
            event => Self::of_contest(event),
        }
    }

    /// Whether FT8/FT4 on a tier whose ADIF mode is `mode` follows `ruleset`'s own messages: FT8
    /// and FT4, where the rules do not ban the mode. Everywhere else the sequencer sends Field
    /// Day's messages as it always has ([`for_contest`](Self::for_contest)).
    pub fn follows_contest(ruleset: &crate::fd_rules::FdRuleset, mode: &str) -> bool {
        let banned = ruleset
            .banned_modes
            .iter()
            .any(|b| b.eq_ignore_ascii_case(mode));
        matches!(mode, "FT8" | "FT4") && !banned
    }

    /// Whether the station on the other end of `ruleset`'s FT8/FT4 contacts on a tier whose ADIF
    /// mode is `mode` runs WSJT-X's Field Day activity: ARRL Field Day on FT8 and FT4. Field
    /// Day's messages anywhere else (FT1, DX1, FT2, Winter Field Day, a mode the rules ban) are
    /// Nexus's own sequencer's and close as they always have.
    pub fn wsjtx_field_day(ruleset: &crate::fd_rules::FdRuleset, mode: &str) -> bool {
        Self::follows_contest(ruleset, mode) && Self::of_contest(ruleset.event) == Self::FieldDay
    }

    /// WSJT-X's own number for this activity, the Special Operation Mode its UDP Status carries
    /// (`NetworkMessage.hpp`: 0 NONE, 1 NA VHF, 3 FIELD DAY; `mainwindow.cpp`'s
    /// `statusUpdate` sends `m_specOp` as it is set).
    pub fn special_op(self) -> u8 {
        match self {
            Self::FieldDay => 3,
            Self::NaVhf => 1,
            Self::Standard => 0,
        }
    }

    /// The CQ a running station with these messages calls from `mycall` (`genCQMsg`,
    /// mainwindow.cpp 9466-9531): `CQ FD` with `mygrid` as Field Day has always sent it,
    /// `CQ TEST` in the ARRL VHF contests, and where WSJT-X has no activity the CQ the operator
    /// typed in Tx6, `dir` its direction ("" for a plain CQ), as WSJT-X's normal mode sends its
    /// Tx6; both with `sent_grid`: the contest's own `session_grid` where it exchanges one, else
    /// `mygrid` cut to the four characters WSJT-X sends.
    ///
    /// ⛔ **A nonstandard `mycall` calls WSJT-X's plain CQ, `CQ <call>`, whatever the messages.**
    /// WSJT-X's `stdCall` (mainwindow.cpp 9541-9552) decides, and for a call that fails it
    /// `genCQMsg` sends `"%1 %2".arg(m_CQtype).arg(my_callsign)` — no grid (9488-9494) — with no
    /// activity token (9506-9510). FT8 cannot carry such a call beside a token or a grid: `CQ FD
    /// PJ4/K1ABC FN42` packs as `CQ FD K1ABC FN42`, another station's call. No direction either:
    /// the packer has no form for one beside it (it would go out as truncated free text).
    pub fn cq(self, mycall: &str, mygrid: &str, session_grid: &str, dir: &str) -> Msg {
        if !crate::message::is_std_call(mycall) {
            return Msg::Cq {
                de: mycall.to_string(),
                grid: String::new(),
                dir: String::new(),
            };
        }
        let (dir, grid) = match self {
            Self::FieldDay => ("FD", mygrid.to_string()),
            Self::NaVhf => ("TEST", sent_grid(session_grid, mygrid)),
            Self::Standard => (dir, sent_grid(session_grid, mygrid)),
        };
        Msg::Cq {
            de: mycall.to_string(),
            grid,
            dir: dir.to_string(),
        }
    }

    /// [`cq`](Self::cq) as it goes on the air: the packer sends four characters of a grid, and
    /// WSJT-X's Tx6 shows four (`genCQMsg`: `grid.left (4)`). What the Operate cockpit's Tx6 row
    /// shows while the contest runs, where WSJT-X has an activity for it (no direction typed).
    pub fn cq_on_air(self, mycall: &str, mygrid: &str, session_grid: &str) -> String {
        let mut cq = self.cq(mycall, mygrid, session_grid, "");
        if let Msg::Cq { grid, .. } = &mut cq {
            *grid = crate::qso::air_grid(grid);
        }
        cq.to_text()
    }
}

/// The grid a contest station sends outside Field Day's messages: the contest's own GRID where it
/// exchanges one (a rover's "I moved" changes it), else the Settings locator cut to four
/// characters, as WSJT-X's `my_grid().left(4)` is.
fn sent_grid(session_grid: &str, mygrid: &str) -> String {
    match session_grid {
        "" => crate::qso::air_grid(mygrid),
        grid => grid.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdState {
    /// Monitoring (search & pounce): answers no CQ by itself. The operator starts each contact
    /// with a double-click on a CQ, or a logger's Reply to one ([`FieldDayStation::answer_cq`]).
    Listening,
    /// Calling CQ FD (running).
    CallingCq,
    /// (S&P) sent my exchange; awaiting the runner's rogered exchange.
    AwaitExchange,
    /// (Running) got a caller's exchange; awaiting their RR73 confirmation.
    AwaitConfirm,
    /// Contact complete (logged).
    Done,
}

/// One station's Field Day auto-sequencer.
#[derive(Debug)]
pub struct FieldDayStation {
    pub mygrid: String,
    pub state: FdState,
    pub pending: Option<Msg>,
    pub dxcall: Option<String>,
    /// A caller's exchange, remembered until their RR73 lets us log it.
    peer_exch: Option<(String, String, String)>, // (call, class, section)
    /// The grid of the station being worked, as it sent it, remembered until the contact is
    /// logged — the received GRID of a contest that exchanges one.
    peer_grid: Option<String>,
    /// What this station puts on the air. Field Day's until the engine, which knows the
    /// contest and the tier, says otherwise ([`set_messages`](Self::set_messages)).
    messages: FtMessages,
    /// The caller runs WSJT-X's Field Day activity ([`FtMessages::wsjtx_field_day`]), so Field
    /// Day's runner answers its RR73 or RRR with 73. Off until the engine says so
    /// ([`set_wsjtx_field_day`](Self::set_wsjtx_field_day)).
    wsjtx_field_day: bool,
    /// The contact in hand is one the operator picked ([`answer_cq`](Self::answer_cq)), not a
    /// caller the run answered. Cleared when the station re-arms.
    picked: bool,
    /// The direction of the operator's Tx6 CQ, called where WSJT-X has no activity
    /// ([`set_cq_dir`](Self::set_cq_dir)); empty for a plain CQ.
    cq_dir: String,
    pub log: FieldDayLog,
    pub transcript: Vec<String>,
}

impl FieldDayStation {
    fn mycall(&self) -> &str {
        &self.log.mycall
    }

    /// A running station (calls CQ FD).
    pub fn running(
        mycall: &str,
        mygrid: &str,
        session: crate::contest::ContestSession,
        band: &str,
    ) -> Self {
        Self {
            mygrid: mygrid.to_string(),
            state: FdState::CallingCq,
            // Field Day's running CQ is the directed "CQ FD <call> <grid>" — the `FD` token
            // advertises a Field Day CQ (`to_text` renders it, `is_cq_dir` accepts it), and a
            // double-click still answers ANY CQ, FD or plain — but a nonstandard call's is
            // WSJT-X's plain `CQ <call>` ([`FtMessages::cq`]).
            pending: Some(FtMessages::FieldDay.cq(mycall, mygrid, "", "")),
            dxcall: None,
            peer_exch: None,
            peer_grid: None,
            messages: FtMessages::FieldDay,
            wsjtx_field_day: false,
            picked: false,
            cq_dir: String::new(),
            log: FieldDayLog::new(mycall, session, band),
            transcript: Vec::new(),
        }
    }

    /// A search-and-pounce station: it listens, and answers the CQ the operator picks
    /// ([`answer_cq`](Self::answer_cq)).
    pub fn search_and_pounce(
        mycall: &str,
        mygrid: &str,
        session: crate::contest::ContestSession,
        band: &str,
    ) -> Self {
        Self {
            mygrid: mygrid.to_string(),
            state: FdState::Listening,
            pending: None,
            dxcall: None,
            peer_exch: None,
            peer_grid: None,
            messages: FtMessages::FieldDay,
            wsjtx_field_day: false,
            picked: false,
            cq_dir: String::new(),
            log: FieldDayLog::new(mycall, session, band),
            transcript: Vec::new(),
        }
    }

    pub fn done(&self) -> bool {
        self.state == FdState::Done && self.pending.is_none()
    }

    pub fn outgoing(&self) -> Option<Msg> {
        self.pending.clone()
    }

    pub fn after_tx(&mut self) {
        if self.state == FdState::Done {
            self.pending = None;
        }
    }

    /// Return a finished station (`Done`, closing frame already sent) to its
    /// starting posture so it works the NEXT contact instead of going silent
    /// after a single QSO — Run → back to calling its CQ, S&P → back to
    /// listening. The contest log is kept, so it remains the dupe/score source.
    pub fn rearm(&mut self, running: bool) {
        self.picked = false;
        self.dxcall = None;
        self.peer_exch = None;
        self.peer_grid = None;
        if running {
            self.state = FdState::CallingCq;
            self.pending = Some(self.cq());
        } else {
            self.state = FdState::Listening;
            self.pending = None;
        }
    }

    /// Put `messages` on the air from now on. A change re-arms the station: a contact begun in
    /// one set of messages cannot be finished in another, so Run goes back to calling its
    /// (new) CQ and S&P back to listening. The same set again changes nothing.
    pub fn set_messages(&mut self, messages: FtMessages, running: bool) {
        if self.messages != messages {
            self.messages = messages;
            self.rearm(running);
        }
    }

    /// What this station puts on the air.
    pub fn messages(&self) -> FtMessages {
        self.messages
    }

    /// Say whether the caller runs WSJT-X's Field Day activity
    /// ([`FtMessages::wsjtx_field_day`]). It decides only how Field Day's runner closes the next
    /// contact it confirms; nothing in flight is re-armed.
    pub fn set_wsjtx_field_day(&mut self, on: bool) {
        self.wsjtx_field_day = on;
    }

    /// Answer `de`'s CQ, which carried `grid` (empty if it carried none): Field Day's exchange,
    /// or my grid where the contest answers with it. This is the operator's double-click on that
    /// CQ, or a logger's Reply to it, and the only way S&P starts a contact; it answers a station
    /// already in the log too: the operator picked it. The engine never hands it a CQ these
    /// messages cannot answer (one with no grid, outside Field Day's).
    pub fn answer_cq(&mut self, de: &str, grid: &str) {
        let answer = match self.messages {
            FtMessages::FieldDay => self.my_exch_msg(de, false),
            FtMessages::NaVhf | FtMessages::Standard => self.grid_answer(de),
        };
        self.dxcall = Some(de.to_string());
        // The standard messages never carry the runner's grid again, so its CQ's is the one
        // logged; the VHF contests log the grid the runner rogers with, Field Day none.
        self.peer_grid = (self.messages == FtMessages::Standard).then(|| grid.to_string());
        self.pending = Some(answer);
        self.state = FdState::AwaitExchange;
        self.picked = true;
        self.transcript.push(format!("answer CQ {de}"));
    }

    /// Whether the contact in hand is one the operator picked ([`answer_cq`](Self::answer_cq)):
    /// "Disable Tx after sending 73" turns TX off after it, as WSJT-X does after a double-clicked
    /// contact, where the run keeps TX on to call CQ again.
    pub fn picked(&self) -> bool {
        self.picked
    }

    /// Whether what this station sends carries its grid: every set of messages but Field
    /// Day's, whose exchange is the class and section. The keying gate asks.
    pub fn sends_grid(&self) -> bool {
        self.messages != FtMessages::FieldDay
    }

    /// The CQ this station calls ([`FtMessages::cq`]).
    fn cq(&self) -> Msg {
        self.messages.cq(
            self.mycall(),
            &self.mygrid,
            self.log.session.field("GRID"),
            &self.cq_dir,
        )
    }

    /// The direction of the operator's Tx6 CQ ("" for a plain CQ), which a running station
    /// calls where WSJT-X has no activity for the contest ([`FtMessages::cq`]). A change rebuilds
    /// the CQ a calling station is about to send, as [`rearm`](Self::rearm) does; nothing else is
    /// touched.
    pub fn set_cq_dir(&mut self, dir: &str) {
        if self.cq_dir != dir {
            self.cq_dir = dir.to_string();
            if self.state == FdState::CallingCq {
                self.pending = Some(self.cq());
            }
        }
    }

    /// The grid this station sends outside Field Day (`sent_grid`).
    fn air_grid(&self) -> String {
        sent_grid(self.log.session.field("GRID"), &self.mygrid)
    }

    /// Addressed to this station: base calls, so a portable call matches.
    fn to_me(&self, to: &str) -> bool {
        crate::message::same_call(to, self.mycall())
    }

    /// Sent by the station being worked: base calls, as [`to_me`](Self::to_me). The name is
    /// the standard sequencer's (`qso::Station::from_dx`), lint allowance and all.
    #[allow(clippy::wrong_self_convention)]
    fn from_dx(&self, de: &str) -> bool {
        self.dxcall
            .as_deref()
            .is_some_and(|dx| crate::message::same_call(dx, de))
    }

    /// Addressed to my call in Field Day's messages: exactly, as always, or — my call being
    /// nonstandard — hashed. Field Day's message carries each call in a 28-bit field, which
    /// holds a nonstandard call only as its hash (`pack28`), so a reply to my exchange names it
    /// `<K1ABC/QRP>`. A standard call is matched exactly, as before.
    fn to_my_call(&self, to: &str) -> bool {
        let me = self.mycall();
        to == me || (!crate::message::is_std_call(me) && crate::message::unhash_call(to) == me)
    }

    /// Addressed to my call in a caller's RR73, RRR or 73: whole, or hashed whatever my call.
    /// A caller whose call is nonstandard closes with mine hashed and its own whole, as WSJT-X
    /// 3.0.2 writes it, `<K1ABC> W9XYZ/M RR73` (`genStdMsgs`, mainwindow.cpp 9702 and 9710), and
    /// WSJT-X takes that as its own: the decoder prints its own call's hash as `<K1ABC>`
    /// (`unpack77`, packjt77.f90 644-651, `n12.eq.hashmy12`), `DecodedText` drops the brackets
    /// (decodedtext.cpp 66), and `auto_sequence` and `processMessage` ask whether that word holds
    /// its call (7546, 9108). The exchanges still take a standard call of mine only whole
    /// ([`to_my_call`](Self::to_my_call)).
    fn closes_to_me(&self, to: &str) -> bool {
        crate::message::unhash_call(to) == self.mycall()
    }

    /// The caller `de` of a first call with no grid, as my report names it: WSJT-X's Tx2 to it
    /// (`genStdMsgs`, mainwindow.cpp), its call hashed where it is nonstandard (`t0a`, 9596,
    /// 9629-9631) and whole otherwise, beside my call as [`my_call_beside`](Self::my_call_beside)
    /// writes it (`t0b`, 9625-9626). `None` for `<...>`, which names nobody (WSJT-X puts only a
    /// callsign into its DX call box, 9402), and for a nonstandard call beside a nonstandard call
    /// of mine, between which no message carries a report.
    fn gridless_caller(&self, de: &str) -> Option<String> {
        let his = crate::message::unhash_call(de);
        if !crate::message::is_callsign(his) {
            return None;
        }
        match (
            crate::message::is_std_call(his),
            crate::message::is_std_call(self.mycall()),
        ) {
            (true, _) => Some(his.to_string()),
            (false, true) => Some(format!("<{his}>")),
            (false, false) => None,
        }
    }

    /// My call as WSJT-X 3.0.2 writes it beside a grid or a report, its Tx2 and Tx3
    /// (`genStdMsgs`, mainwindow.cpp): whole when `stdCall` passes it (`/P` and `/R` too), else
    /// hashed, `t0b=hisCall + " <" + my_callsign + "> "` (9597, taken by `if(!bMyCall) { t=t0b;`
    /// at 9625-9626). Whole, the packer would send the base call: `W9XYZ PJ4/K1ABC FN42` goes
    /// out as `W9XYZ K1ABC FN42`, another station.
    fn my_call_beside(&self) -> String {
        let me = self.mycall();
        if crate::message::is_std_call(me) {
            me.to_string()
        } else {
            format!("<{me}>")
        }
    }

    /// The two calls of an over to `to` that carries no grid, report or exchange — RR73, 73,
    /// and the standard messages' answer to a CQ: as they always were with a standard call of
    /// mine, and with a nonstandard one `to` hashed and mine whole, as WSJT-X writes them:
    /// `t0a="<"+hisCall + "> " + my_callsign + " "` (9596), the answer `if(!bMyCall) t=t0a;`
    /// (9601), RR73 and 73 `if(bHisCall and !bMyCall) t="<" + hisCall + "> " + my_callsign …`
    /// (9702, 9710).
    ///
    /// ⭐ RR73 and 73 from a standard call of mine to a nonstandard `to` (`W9XYZ/M`, `/4`,
    /// `/QRP`, `PJ4/W9XYZ`) go `to` whole and mine hashed, as WSJT-X writes them in every set of
    /// messages (its Tx4 and Tx5 are the same lines in every activity): `if(!bHisCall and
    /// bMyCall) t=hisCall + " <" + my_callsign + "> " …` (9701, 9709), `W9XYZ/M <K1ABC> RR73`, a
    /// type 4 message. `hisCall` is the call the decoder printed, brackets dropped
    /// (decodedtext.cpp 66): such a station sends its exchange, grid or report with its call
    /// hashed (`K1ABC <W9XYZ/M> R 1D WI`) and closes with it whole (`<K1ABC> W9XYZ/M RR73`), and
    /// either way it is the same call here. Hashed the other way round, as Field Day's and the
    /// standard messages' RR73 went before, `<W9XYZ/M> K1ABC RR73` is a type 1 message, not
    /// WSJT-X's; whole beside my call whole, as the ARRL VHF and standard messages' 73 went
    /// before, the packer sends the base call, `W9XYZ K1ABC 73`, another station.
    fn closing_calls(&self, to: &str) -> (String, String) {
        let me = self.mycall().to_string();
        let his = crate::message::unhash_call(to);
        if crate::message::is_std_call(&me) && crate::message::is_nonstandard_call(his) {
            return (his.to_string(), format!("<{me}>"));
        }
        if crate::message::is_std_call(&me) {
            (to.to_string(), me)
        } else {
            (format!("<{his}>"), me)
        }
    }

    /// RR73 to `to` (WSJT-X's Tx4), its calls as [`closing_calls`](Self::closing_calls) writes
    /// them.
    fn rr73(&self, to: &str) -> Msg {
        let (to, de) = self.closing_calls(to);
        Msg::Rr73 { to, de }
    }

    /// 73 to `to` (WSJT-X's Tx5), its calls as [`closing_calls`](Self::closing_calls) writes
    /// them.
    fn bye73(&self, to: &str) -> Msg {
        let (to, de) = self.closing_calls(to);
        Msg::Bye73 { to, de }
    }

    /// My answer to `to`'s CQ outside Field Day's messages: my grid, WSJT-X's Tx2 in the ARRL
    /// VHF contests (whose Tx1 is switched off, 8238-8248) and its Tx1 in the standard messages.
    /// A nonstandard call goes hashed beside the grid in the Tx2
    /// ([`my_call_beside`](Self::my_call_beside)), and whole in the Tx1, which is then `t0a`
    /// alone: no grid, which that message has no room for (`msgtype(t0a, ui->tx1)`, 9627).
    fn grid_answer(&self, to: &str) -> Msg {
        match self.messages {
            FtMessages::Standard if !crate::message::is_std_call(self.mycall()) => {
                let (to, de) = self.closing_calls(to);
                Msg::Grid {
                    to,
                    de,
                    grid: String::new(),
                }
            }
            _ => Msg::Grid {
                to: to.to_string(),
                de: self.my_call_beside(),
                grid: self.air_grid(),
            },
        }
    }

    /// The contest's fields a contact with a station on `grid` exchanges: its GRID received
    /// and mine sent where the contest exchanges one, the way a typed entry records them, and
    /// nothing otherwise — the standard messages carry none of the exchange. One builder for
    /// the row [`log_worked`](Self::log_worked) writes.
    fn exchanged(
        &self,
        grid: Option<&str>,
    ) -> (
        Vec<crate::contest::FieldValue>,
        Vec<crate::contest::FieldValue>,
    ) {
        let rx = grid
            .and_then(|g| self.log.session.exchange.copied("GRID", g))
            .into_iter()
            .collect();
        let tx = self
            .log
            .session
            .my_exchange
            .iter()
            .filter(|v| v.key == "GRID")
            .cloned()
            .collect();
        (rx, tx)
    }

    /// Log the contact with `call`, whose grid was `grid` ([`exchanged`](Self::exchanged)). A
    /// call that came hashed is logged without the brackets: a station answering a nonstandard
    /// call of mine sends its RR73 or 73 with its own call hashed, `PJ4/K1ABC <W9XYZ> RR73`.
    fn log_worked(&mut self, call: &str, grid: Option<&str>, slot: u64) {
        let call = crate::message::unhash_call(call);
        let (rx, tx) = self.exchanged(grid);
        let submode = self.log.current_submode.clone();
        self.log
            .log_exchange_at(call, rx, tx, "DIG", &submode, slot, now_unix());
        self.transcript
            .push(format!("logged {call} {}", grid.unwrap_or_default()));
    }

    /// Log the contact with `call`, who sent `class` and `section` in Field Day's exchange. A
    /// call that came hashed is logged without the brackets, as [`log_worked`](Self::log_worked)
    /// logs it: a nonstandard call packs its Field Day exchange with the call hashed (`pack28`),
    /// `K1ABC <W9XYZ/M> R 1D WI`, and WSJT-X logs the call in its DX call box, which the decoded
    /// message fills with the brackets dropped (`DecodedText`, decodedtext.cpp 66).
    fn log_field_day(&mut self, call: &str, class: &str, section: &str, slot: u64) {
        let call = crate::message::unhash_call(call);
        self.log.log(call, class, section, slot);
    }

    /// My Field Day exchange to `to`, rogered or not (WSJT-X's Tx3 and Tx2).
    ///
    /// ⚠️ **A nonstandard call goes in whole, not as WSJT-X writes it.** Field Day's packer
    /// hashes a nonstandard call itself (`pack28`), so `W9XYZ K1ABC/QRP 2B EMA` goes out
    /// bit for bit as WSJT-X's `W9XYZ <K1ABC/QRP> 2B EMA` (`t0b`, mainwindow.cpp 9597) does. A
    /// prefixed call it refuses bracketed: `chkcall` reads `K1ABC>` as the base call, so
    /// WSJT-X 3.0.2's own `W9XYZ <PJ4/K1ABC> 2B EMA` goes out as the free text `W9XYZ  PJ4/K1`,
    /// where `W9XYZ PJ4/K1ABC 2B EMA` goes out as the exchange with the call hashed.
    fn my_exch_msg(&self, to: &str, roger: bool) -> Msg {
        Msg::FieldDay {
            to: to.to_string(),
            de: self.mycall().to_string(),
            roger,
            // What is going on the air RIGHT NOW is exactly what the session holds,
            // so this reads the session — and it is the one reader that legitimately
            // does. `Msg::FieldDay` has typed class and section fields of its own, so
            // no exchange is rendered here; a reader DESCRIBING a past contact reads
            // that contact's row instead (`contest::sent_exchange`).
            class: self.log.session.field("CLASS").to_string(),
            section: self.log.session.field("SECTION").to_string(),
        }
    }

    /// Process the signals decoded this slot and advance the exchange.
    pub fn observe(&mut self, decodes: &[Decode], slot: u64) {
        match self.messages {
            FtMessages::NaVhf => return self.observe_na_vhf(decodes, slot),
            FtMessages::Standard => return self.observe_standard(decodes, slot),
            FtMessages::FieldDay => {}
        }
        for d in decodes {
            // ⭐ A message naming a call the decoder could not resolve is passed by, as WSJT-X's
            // Field Day activity passes it by: for a message to this station, `processMessage`
            // returns at once where it carries `<...>` (mainwindow.cpp 9097-9099, inside the
            // branch for a message to it that opens at 9050); for a CQ from such a call, only a
            // callsign goes into the DX call box (`Radio::is_callsign`, 9402), so no message is
            // built to it. The run would answer that caller as `<...>` and log `...`.
            if d.message.contains("<...>") {
                continue;
            }
            let m = Msg::parse(&d.message);
            match (self.state, &m) {
                // NOTE: there is intentionally NO (Listening, Cq) auto-answer arm. S&P listens: it
                // must NEVER start a contact on its own (contests require the operator to start
                // each one, and WSJT-X calls nobody the operator did not pick). The operator
                // starts one by double-clicking a CQ, or a logger replies to one, through
                // [`answer_cq`](Self::answer_cq).
                // Running: a caller sent their exchange → roger + send mine.
                (
                    FdState::CallingCq,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: false,
                        class,
                        section,
                    },
                ) if self.to_my_call(to) => {
                    self.dxcall = Some(de.clone());
                    self.peer_exch = Some((de.clone(), class.clone(), section.clone()));
                    self.pending = Some(self.my_exch_msg(de, true));
                    self.state = FdState::AwaitConfirm;
                    self.transcript
                        .push(format!("caller {de} {class} {section} → R + my exch"));
                }
                // Running (out-of-order tolerance): a caller that ROGERED its
                // exchange (roger:true) while we're still calling CQ — the plain
                // exchange was dropped, or the caller pre-rogered. The class +
                // section are carried in the rogered frame, so skip straight to
                // logging and close with RR73 rather than stalling on CQ.
                // Happy-path callers send roger:false, so this never fires on the
                // normal sequence.
                (
                    FdState::CallingCq,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: true,
                        class,
                        section,
                    },
                ) if self.to_my_call(to) => {
                    self.dxcall = Some(de.clone());
                    self.log_field_day(de, class, section, slot);
                    self.pending = Some(self.rr73(de));
                    self.state = FdState::Done;
                    self.transcript.push(format!(
                        "caller {de} {class} {section} rogered early → log + RR73"
                    ));
                }
                // S&P: the runner rogered + sent their exchange → log + RR73.
                //
                // ⭐ Only the station called, by its call, as WSJT-X acts only on its QSO
                // partner's message (`auto_sequence`, mainwindow.cpp 7546-7549, and
                // `processMessage`, 9107-9110: the sender must be the DX call). A rogered exchange
                // to us from anyone else logs nothing and sends nothing back, and the station
                // called keeps getting our exchange.
                (
                    FdState::AwaitExchange,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: true,
                        class,
                        section,
                    },
                ) if self.to_my_call(to) && self.from_dx(de) => {
                    self.log_field_day(de, class, section, slot);
                    self.pending = Some(self.rr73(de));
                    self.state = FdState::Done;
                    self.transcript
                        .push(format!("logged {de} {class} {section}; send RR73"));
                }
                // Running: caller confirmed → log them. A bare `73` (Bye73)
                // completes too — if the caller closes with 73 instead of
                // RR73/RRR the contact must not stall (their exchange is already
                // in hand from the CallingCq step). Happy-path callers send RR73,
                // so the added Bye73 alternative never fires on the normal flow.
                //
                // ⭐ Only the caller's, by its call, as WSJT-X acts only on its QSO partner's
                // message (`processMessage`, mainwindow.cpp 9107-9110: the sender must be the
                // DX call). An RR73, RRR or 73 to us from anyone else logs nothing and sends
                // nothing back, and the caller keeps getting our R and exchange.
                //
                // ⭐ Where the caller runs WSJT-X's Field Day activity, its RR73 or RRR is
                // answered with 73, as WSJT-X's runner answers it (Tx5, mainwindow.cpp
                // 9210-9215); the 73 goes once and the run calls CQ again. A bare 73, and every
                // other Field Day station, close as they always have, with nothing sent back.
                //
                // ⭐ A caller whose call is nonstandard closes with my call hashed
                // ([`closes_to_me`](Self::closes_to_me)), and is answered with WSJT-X's 73 to it
                // ([`closing_calls`](Self::closing_calls)).
                (FdState::AwaitConfirm, Msg::Rr73 { to, de })
                | (FdState::AwaitConfirm, Msg::Rrr { to, de })
                | (FdState::AwaitConfirm, Msg::Bye73 { to, de })
                    if self.closes_to_me(to) && self.from_dx(de) =>
                {
                    if let Some((call, class, section)) = self.peer_exch.take() {
                        self.log_field_day(&call, &class, &section, slot);
                        self.transcript
                            .push(format!("logged {call} {class} {section}"));
                    }
                    self.pending = match &m {
                        Msg::Rr73 { de, .. } | Msg::Rrr { de, .. } if self.wsjtx_field_day => {
                            Some(self.bye73(de))
                        }
                        _ => None,
                    };
                    self.state = FdState::Done;
                }
                _ => {}
            }
        }
    }

    /// [`observe`](Self::observe) in the ARRL VHF contests: WSJT-X 3.0.2's NA VHF Contest
    /// sequence (`mainwindow.cpp`). A CQ is answered with the grid, because Tx1 is switched
    /// off and Tx2 carries the grid (8238-8248, 9633); a caller's grid is rogered with mine as
    /// `R <grid>` (Tx3, 9111-9116); `R <grid>` is closed with RR73 (Tx4, 9250-9255); and the
    /// runner answers RR73 or RRR with 73 (Tx5, 9210-9215).
    fn observe_na_vhf(&mut self, decodes: &[Decode], slot: u64) {
        for d in decodes {
            let m = Msg::parse(&d.message);
            match (self.state, &m) {
                // NOTE: there is intentionally NO (Listening, Cq) auto-answer arm. S&P listens: it
                // must NEVER start a contact on its own (contests require the operator to start
                // each one, and WSJT-X calls nobody the operator did not pick). The operator
                // starts one by double-clicking a CQ, or a logger replies to one, through
                // [`answer_cq`](Self::answer_cq).
                // Running: a caller's grid → roger it and send mine. Not a caller from `<...>`,
                // which names nobody: WSJT-X's messages go to the call in its DX call box
                // (`genStdMsgs`, mainwindow.cpp 9567-9577), which takes only a callsign (9402).
                (FdState::CallingCq, Msg::Grid { to, de, grid })
                    if self.to_me(to) && !grid.is_empty() && !de.contains("<...>") =>
                {
                    self.dxcall = Some(de.clone());
                    self.peer_grid = Some(grid.clone());
                    self.pending = Some(rogered_grid(de, &self.my_call_beside(), &self.air_grid()));
                    self.state = FdState::AwaitConfirm;
                    self.transcript
                        .push(format!("caller {de} {grid} → R + my grid"));
                }
                // Running: the caller confirmed → log them, and close with 73.
                (FdState::AwaitConfirm, Msg::Rr73 { to, de } | Msg::Rrr { to, de })
                    if self.to_me(to) && self.from_dx(de) =>
                {
                    let grid = self.peer_grid.take();
                    self.log_worked(de, grid.as_deref(), slot);
                    self.pending = Some(self.bye73(de));
                    self.state = FdState::Done;
                }
                // S&P: the runner rogered and sent its grid → log it, and close with RR73.
                (FdState::AwaitExchange, Msg::Other(text)) => {
                    let Some((to, de, grid)) = rogered_grid_of(text) else {
                        continue;
                    };
                    if !self.to_me(&to) || !self.from_dx(&de) {
                        continue;
                    }
                    self.log_worked(&de, Some(&grid), slot);
                    self.pending = Some(self.rr73(&de));
                    self.state = FdState::Done;
                }
                _ => {}
            }
        }
    }

    /// [`observe`](Self::observe) in a contest WSJT-X has no activity for: the standard
    /// sequence `processMessage` runs with none (`mainwindow.cpp`). A CQ is answered with the
    /// grid (Tx1), a grid with a report (Tx2, 9134), a report with `R` and mine (Tx3, 9283), `R`
    /// and a report with RR73 (Tx4, 9250-9255), and RR73 or RRR with 73 (Tx5, 9210-9215). The
    /// report sent is the SNR the other station was decoded at, as WSJT-X's is.
    fn observe_standard(&mut self, decodes: &[Decode], slot: u64) {
        for d in decodes {
            let m = Msg::parse(&d.message);
            let rpt = d.snr.clamp(-30, 49);
            match (self.state, &m) {
                // NOTE: there is intentionally NO (Listening, Cq) auto-answer arm. S&P listens: it
                // must NEVER start a contact on its own (contests require the operator to start
                // each one, and WSJT-X calls nobody the operator did not pick). The operator
                // starts one by double-clicking a CQ, or a logger replies to one, through
                // [`answer_cq`](Self::answer_cq).
                // Running: a caller's grid → my report. Not a caller from `<...>`, as in the ARRL
                // VHF contests.
                (FdState::CallingCq, Msg::Grid { to, de, grid })
                    if self.to_me(to) && !grid.is_empty() && !de.contains("<...>") =>
                {
                    self.dxcall = Some(de.clone());
                    self.peer_grid = Some(grid.clone());
                    self.pending = Some(Msg::Report {
                        to: de.clone(),
                        de: self.my_call_beside(),
                        snr: rpt,
                    });
                    self.state = FdState::AwaitExchange;
                    self.transcript.push(format!("caller {de} {grid} → report"));
                }
                // Running: a caller's first call that carries no grid → my report, as WSJT-X
                // running with no activity answers a message to it of fewer than five words with
                // Tx2 (`processMessage`, mainwindow.cpp 9315-9325). A caller whose call is
                // nonstandard sends its first call that way, `<K1ABC> W9XYZ/M` (`msgtype(t0a,
                // ui->tx1)`, 9627). The report's calls: [`gridless_caller`](Self::gridless_caller).
                (FdState::CallingCq, Msg::Grid { to, de, grid })
                    if self.to_me(to) && grid.is_empty() =>
                {
                    let Some(his) = self.gridless_caller(de) else {
                        continue;
                    };
                    self.dxcall = Some(de.clone());
                    self.peer_grid = None;
                    self.pending = Some(Msg::Report {
                        to: his,
                        de: self.my_call_beside(),
                        snr: rpt,
                    });
                    self.state = FdState::AwaitExchange;
                    self.transcript
                        .push(format!("caller {de} with no grid → report"));
                }
                // Running: a caller that sent its report without its grid → R and mine. Not a
                // caller from `<...>`.
                (FdState::CallingCq, Msg::Report { to, de, .. })
                    if self.to_me(to) && !de.contains("<...>") =>
                {
                    self.dxcall = Some(de.clone());
                    self.pending = Some(Msg::RReport {
                        to: de.clone(),
                        de: self.my_call_beside(),
                        snr: rpt,
                    });
                    self.state = FdState::AwaitConfirm;
                    self.transcript
                        .push(format!("caller {de} report → R + report"));
                }
                // The report after my grid → R and mine.
                (FdState::AwaitExchange, Msg::Report { to, de, .. })
                    if self.to_me(to) && self.from_dx(de) =>
                {
                    self.pending = Some(Msg::RReport {
                        to: de.clone(),
                        de: self.my_call_beside(),
                        snr: rpt,
                    });
                    self.state = FdState::AwaitConfirm;
                }
                // `R` and a report after my report → log, and close with RR73.
                (FdState::AwaitExchange, Msg::RReport { to, de, .. })
                    if self.to_me(to) && self.from_dx(de) =>
                {
                    let grid = self.peer_grid.take();
                    self.log_worked(de, grid.as_deref(), slot);
                    self.pending = Some(self.rr73(de));
                    self.state = FdState::Done;
                }
                // RR73 or RRR after my `R` and report → log, and close with 73.
                (FdState::AwaitConfirm, Msg::Rr73 { to, de } | Msg::Rrr { to, de })
                    if self.to_me(to) && self.from_dx(de) =>
                {
                    let grid = self.peer_grid.take();
                    self.log_worked(de, grid.as_deref(), slot);
                    self.pending = Some(self.bye73(de));
                    self.state = FdState::Done;
                }
                _ => {}
            }
        }
    }
}

/// `<to> <de> R <grid>` — WSJT-X's Tx3 in its grid contests (`genStdMsgs`, mainwindow.cpp
/// 9657), a standard (i3=1) message with the R bit set.
///
/// ⚠️ Built and read HERE rather than taught to [`Msg`]: `Msg::parse` reads every decode in the
/// app, outside any contest too, and a new form there would change how the band activity, the
/// roster and the alerts read a message while no contest is running. The sequencer that sends
/// this message is the one thing that needs to read it.
fn rogered_grid(to: &str, de: &str, grid: &str) -> Msg {
    Msg::Other(format!("{to} {de} R {grid}"))
}

/// The `(to, de, grid)` of a [`rogered_grid`] message, or `None`: two callsigns, `R`, and a
/// four-character locator in the packer's own form — letters `A` to `R`, then two digits —
/// that is not `RR73` (`packjt77.f90` `pack77_1`'s `is_grid4`). The first call may be hashed,
/// as the decoder prints a nonstandard call the runner's message carries: `<PJ4/K1ABC> W9XYZ R
/// EN37` is the R and grid to `PJ4/K1ABC`.
fn rogered_grid_of(text: &str) -> Option<(String, String, String)> {
    let t: Vec<&str> = text.split_whitespace().collect();
    let &[to, de, "R", grid] = t.as_slice() else {
        return None;
    };
    let b = grid.as_bytes();
    let is_grid = b.len() == 4
        && grid != "RR73"
        && (b'A'..=b'R').contains(&b[0])
        && (b'A'..=b'R').contains(&b[1])
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit();
    (is_grid && crate::message::looks_like_call(to) && crate::message::is_callsign(de))
        .then(|| (to.to_string(), de.to_string(), grid.to_string()))
}

/// Run a Field Day exchange between a running and an S&P station over the
/// virtual channel. Stops when both have logged the contact or `max_slots`.
/// S&P answers no CQ by itself, so the S&P operator double-clicks the first CQ
/// it decodes ([`FieldDayStation::answer_cq`]).
pub fn run_loopback_fieldday(
    running: &mut FieldDayStation,
    sp: &mut FieldDayStation,
    snr_db: f32,
    max_slots: u64,
) {
    use crate::channel::{to_i16, VirtualAir, ON_TIME_OFFSET};
    use crate::tx;

    let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, 0xFD0001);
    for slot in 0..max_slots {
        let (txs, rxs): (&mut FieldDayStation, &mut FieldDayStation) = if slot % 2 == 0 {
            (&mut *running, &mut *sp)
        } else {
            (&mut *sp, &mut *running)
        };
        if let Some(msg) = txs.outgoing() {
            let text = msg.to_text();
            let frame = tx::build(&text, tempo_fast::SAMPLE_RATE, 1500.0);
            let rx_f32 = air.receive(&frame.wave, ON_TIME_OFFSET, snr_db);
            let decodes: Vec<Decode> = tempo_fast::decode_frame(
                &to_i16(&rx_f32),
                200,
                2900,
                3,
                "",
                "",
                0,
                (slot as i64).wrapping_mul(4000), // monotonic ms for IR-HARQ keying
            )
            .into_iter()
            .map(Into::into)
            .collect();
            rxs.observe(&decodes, slot);
            if slot % 2 == 0 && rxs.state == FdState::Listening {
                let cq = decodes.iter().find_map(|d| match Msg::parse(&d.message) {
                    Msg::Cq { de, grid, .. } => Some((de, grid)),
                    _ => None,
                });
                if let Some((de, grid)) = cq {
                    rxs.answer_cq(&de, &grid);
                }
            }
            txs.after_tx();
        }
        if running.log.qso_count() >= 1 && sp.log.qso_count() >= 1 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    /// ⚠️ THE CONTEST LOG *HAS* A PER-CONTACT BAND — what it has no way to do
    /// is let a CALLER name it. Pinned because the Field Day guide said "the
    /// Field Day log has no per-contact band field", which is false and sends
    /// a future fixer looking for a field to add rather than a parameter.
    ///
    /// `LoggedQso::band` is real and is written per contact, so one log can
    /// hold two bands and the exports get them right. But every entry point —
    /// `log`, `log_at`, `log_mode_at`, `log_submode_at` — funnels into
    /// `log_submode_at`, which stamps `self.band`, the log's ONE current band.
    /// No signature takes a band. That is why `Engine::fd_log_manual` opening
    /// with `sync_fd_band()` makes a catch-up entry file on the dial you are on
    /// NOW: not a missing field, an unreachable one.
    ///
    /// ⭐ **ONE CALLER CAN NOW NAME IT, AND ONLY ONE** ([`FieldDayLog::log_satellite_at`],
    /// 2026-09-22 — the satellite half of the caveat this test was the cue for is
    /// deleted from docs/guide/satellites.md). Everything above still holds for every
    /// other path, which is what this test now pins: the exception is an EXCEPTION, and
    /// an entry point that quietly started reading a band from somewhere else would go
    /// red here. The contesting-pota.md caveat is untouched and still true — a POTA/FD
    /// catch-up entry has no pass to take a band from.
    ///
    /// Its twin is `a_satellite_row_keeps_the_birds_band_after_the_radio_has_moved_on`,
    /// directly below: together they say exactly which paths take the log's band and
    /// which does not.
    #[test]
    fn every_contact_carries_a_band_and_it_is_always_the_logs_own() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "70cm",
        );
        assert!(log.log_mode_at("W1AW", "1D", "IL", "PH", 0, 100));
        // The only way the band moves: the LOG's band, not an argument.
        log.band = "20m".into();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "PH", 0, 200));

        let bands: Vec<&str> = log.qsos().iter().map(|q| q.band.as_str()).collect();
        assert_eq!(
            bands,
            ["70cm", "20m"],
            "the per-contact band field stopped recording the log's band at log time"
        );
    }

    /// ⭐ **THE ONE ENTRY POINT WHOSE CALLER NAMES THE BAND** — the exception the test
    /// above exists to make visible, and the reason it is worded "every entry point
    /// funnels into `log_submode_at`" rather than "no row can carry another band".
    ///
    /// A satellite QSO's band is a property of the transponder leg, and the operator
    /// writes the contact up when their hands are free — which on a hand-turned rotator
    /// is after LOS, with the radio already back on the 20 m run. Taking the band from
    /// the log at that moment files a 70 cm pass in the contest log, and out to N1MM and
    /// N3FJP, on 20 m.
    ///
    /// Asserted BY VALUE on the row, and against a log whose own band DISAGREES: an
    /// assertion made with the log still sitting on 70 cm could not tell the fix from
    /// the bug.
    #[test]
    fn a_satellite_row_keeps_the_birds_band_after_the_radio_has_moved_on() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "70cm",
        );
        // The pass is over and the operator is back on the 20 m run: this is what
        // `Engine::sync_fd_band` would have stamped by the time they type the contact.
        log.band = "20m".into();
        log.dial_khz = 14_250;
        log.on_air_hz = 14_250_000;

        let leg = SatLeg {
            band: "70cm".into(),
            down_hz: 435_643_320,
            bird: "RS-44".into(),
            name: Some("RS-44".into()),
            single_channel_fm: false,
        };
        assert!(log.log_satellite_at(
            "W1AW",
            &[
                ("CLASS".into(), "1D".into()),
                ("SECTION".into(), "IL".into()),
            ],
            "PH",
            "USB",
            &leg,
            200,
        ));

        let q = log.qsos().last().expect("the contact entered the log");
        assert_eq!(q.band, "70cm", "the row took its band off the DIAL");
        assert_eq!(
            q.on_air_hz, 435_643_320,
            "the row took its frequency off the dial too"
        );
        assert_eq!(q.freq_khz, 435_643, "…and so did the Cabrillo dial");
        // A satellite contact is not a same-band split: `Engine::log_frequencies`
        // returns the receive leg ALONE for a cross-band pair, and a `FREQ_RX` equal to
        // nothing the operator listened on is a claim carried into every logger.
        assert_eq!(q.freq_rx_hz, None);
        assert_eq!(
            q.sat.as_ref().and_then(|s| s.name.as_deref()),
            Some("RS-44"),
            "the row forgot which bird it was worked through"
        );

        // The 20 m run is untouched by the exception: the very next ordinary contact is
        // stamped from the log again.
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "PH", 0, 300));
        assert_eq!(log.qsos().last().unwrap().band, "20m");
        assert_eq!(log.qsos().last().unwrap().sat, None);
    }

    /// A Field Day exchange as the field vector the satellite path logs.
    fn fd_ex(class: &str, section: &str) -> Vec<(String, String)> {
        vec![
            ("CLASS".into(), class.into()),
            ("SECTION".into(), section.into()),
        ]
    }

    /// RS-44's linear transponder: a passband, so the sponsor's one-QSO limit does not
    /// apply to it.
    fn rs44() -> SatLeg {
        SatLeg {
            band: "70cm".into(),
            down_hz: 435_643_320,
            bird: "RS-44".into(),
            name: Some("RS-44".into()),
            single_channel_fm: false,
        }
    }

    /// SO-50's FM voice repeater: ONE channel, which is the case ARRL limits.
    fn so50() -> SatLeg {
        SatLeg {
            band: "70cm".into(),
            down_hz: 436_795_000,
            bird: "SAUDISAT 1C (SO-50)".into(),
            name: Some("SO-50".into()),
            single_channel_fm: true,
        }
    }

    /// ⭐ **A BIRD IS ITS OWN BAND** — ARRL Field Day, rule 7.3.8: *"Satellite QSOs also
    /// count for regular QSO credit. Show them listed separately on the summary sheet as
    /// a separate 'band.'"*
    ///
    /// So a station worked on 70 cm terrestrially and again through a 70 cm bird is two
    /// legitimate contacts, and both must count. Before the rules data named the
    /// satellite dimension the second one collided with the first on `(call, band, mode
    /// class)` and was REFUSED — a legal QSO the club never got, which is the worse of
    /// the two directions this module's header weighs.
    ///
    /// ⚠️ **The two contacts are deliberately identical in every OTHER component.** Same
    /// call, same band, same mode class — a test whose two rows already differed
    /// somewhere else would pass with the satellite dimension absent and prove nothing.
    #[test]
    fn a_bird_is_its_own_band_so_a_terrestrial_contact_does_not_block_it() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "70cm",
        );
        // 70 cm phone, terrestrial — the club's UHF station.
        assert!(log.log_mode_at("W1AW", "1D", "IL", "PH", 0, 100));
        // The SAME station, the SAME band, the SAME mode class, through RS-44.
        assert!(
            log.log_satellite_at("W1AW", &fd_ex("1D", "IL"), "PH", "USB", &rs44(), 200),
            "the satellite contact was refused as a dupe of the terrestrial one — \
             ARRL lists a bird as a separate band"
        );
        assert_eq!(log.qso_count(), 2, "one of the two contacts scored nothing");

        // …and the dimension still BINDS: the same bird again on the same mode is the
        // dupe it always was. Without this the test above would pass on a rule that had
        // simply stopped checking.
        assert!(
            logged_dupe(
                log.log_satellite_at("W1AW", &fd_ex("1D", "IL"), "PH", "USB", &rs44(), 300),
                &log
            ),
            "a second contact through the same bird on the same mode is not a dupe"
        );
        // A DIFFERENT bird is a different contact, which the sponsor allows: "the
        // additional QSOs may be counted for QSO credit".
        let mut ao7 = rs44();
        ao7.bird = "AO-7".into();
        ao7.name = Some("AO-7".into());
        assert!(log.log_satellite_at("W1AW", &fd_ex("1D", "IL"), "PH", "USB", &ao7, 400));
        assert_eq!(log.qso_count(), 3);
    }

    /// ⭐ **ONE QSO PER SINGLE-CHANNEL FM SATELLITE** — ARRL Field Day: *"Stations are
    /// limited to one (1) completed QSO on any single channel FM satellite."*
    ///
    /// The rule above already refuses a repeat through one bird in one mode class. This
    /// is the part it does not reach: on ONE channel there is no second mode to work the
    /// same station in, so the mode class leaves the key entirely for an FM bird.
    ///
    /// ⚠️ **The control is the whole test.** A linear transponder must still take both,
    /// because the sponsor says so in the same breath — a limit applied to every bird
    /// would silently refuse a legal contact, and the contest log is its only copy.
    #[test]
    fn a_single_channel_fm_bird_takes_one_qso_per_station_and_a_linear_one_does_not() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "70cm",
        );
        // FM voice through SO-50 …
        assert!(log.log_satellite_at("W1AW", &fd_ex("1D", "IL"), "PH", "FM", &so50(), 100));
        // … then the same station again on the same channel in a digital mode. One
        // channel, one QSO.
        assert!(
            logged_dupe(
                log.log_satellite_at("W1AW", &fd_ex("1D", "IL"), "DIG", "FT8", &so50(), 200),
                &log
            ),
            "a second QSO with the same station on a single-channel FM bird is not a dupe"
        );
        assert_eq!(log.qso_count(), 1);

        // ⭐ THE CONTROL, on the same log and the same station: RS-44 is a LINEAR
        // transponder, so two mode classes are two contacts and ARRL counts both. A
        // limit that reddened this is over-reporting, which refuses a legal QSO.
        assert!(log.log_satellite_at("W1AW", &fd_ex("1D", "IL"), "PH", "USB", &rs44(), 300));
        assert!(
            log.log_satellite_at("W1AW", &fd_ex("1D", "IL"), "CW", "", &rs44(), 400),
            "the one-QSO limit reached a linear transponder, where the sponsor allows \
             the additional contact"
        );
        assert_eq!(log.qso_count(), 3);
    }

    /// The satellite pair reaches the JOURNAL and comes back — a mid-event restart is
    /// the ordinary case at a generator-powered site, and `FieldDayLog::adif` is both
    /// the journal and the submitted file.
    ///
    /// ⚠️ **BOTH FIELDS OR NEITHER.** TQSL validates `PROP_MODE`/`SAT_NAME` as a pair
    /// and hard-errors on a lone member, which through the `-a compliant` funnel wedges
    /// the whole upload batch as Rejected.
    #[test]
    fn a_satellite_row_keeps_its_bird_across_a_restart() {
        let session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        let leg = SatLeg {
            band: "70cm".into(),
            down_hz: 435_643_320,
            bird: "RS-44".into(),
            name: Some("RS-44".into()),
            single_channel_fm: false,
        };
        assert!(log.log_satellite_at(
            "W1AW",
            &[
                ("CLASS".into(), "1D".into()),
                ("SECTION".into(), "IL".into()),
            ],
            "PH",
            "USB",
            &leg,
            1_782_583_500,
        ));
        let journal = log.adif();
        assert!(journal.contains("<PROP_MODE:3>SAT"), "{journal}");
        assert!(journal.contains("<SAT_NAME:5>RS-44"), "{journal}");

        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        restored.merge_adif(&journal, 0);
        let q = restored.qsos().last().expect("the row came back");
        assert_eq!(q.band, "70cm");
        assert_eq!(
            q.sat.as_ref().and_then(|s| s.name.as_deref()),
            Some("RS-44"),
            "the restart lost the bird, so the merged lifetime record loses LoTW satellite credit"
        );
    }

    #[test]
    fn arrl_scoring_per_mode_and_band_mode_dupes() {
        // ARRL FD: phone 1 pt, CW/digital 2 pts; a station counts once per
        // band PER MODE CLASS (the old (call, band) dupe key under-counted —
        // K1ABC on 20m CW and 20m FT8 are two legal contacts).
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 100));
        assert!(
            log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, 110),
            "same call, new mode"
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "PH", 0, 120));
        assert!(
            logged_dupe(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 130), &log),
            "band+mode dupe"
        );
        assert!(
            logged_dupe(log.log_mode_at("k1abc", "2A", "CT", "dig", 0, 140), &log),
            "case-insensitive dupe"
        );
        assert_eq!(log.qso_count(), 3);
        assert_eq!(log.qso_points(), 2 + 2 + 1, "DIG 2 + CW 2 + PH 1");
        // Cabrillo mode tokens per row.
        let cab = log
            .cabrillo(14074)
            .expect("a single-mode event exports one entry");
        assert!(cab.contains(" DG "));
        assert!(cab.contains(" CW "));
        assert!(cab.contains(" PH "));
        // The rules-data stamp (Cabrillo-legal X- header, robots ignore it).
        assert!(cab.contains("X-NEXUS-RULES-YEAR: 2026\n"));
        // WFD event flips the contest ids in both exports.
        log.set_event(FdEvent::WinterFd);
        assert!(log
            .cabrillo(14074)
            .expect("a single-mode event exports one entry")
            .contains("CONTEST: WFD"));
        assert!(log.adif().contains("WFD"));
    }

    #[test]
    fn cabrillo_lines_carry_real_utc_timestamps() {
        // 2026-06-27 18:05:00 UTC (Field Day Saturday) = 1782583500.
        let (d, t) = cabrillo_datetime(1_782_583_500);
        assert_eq!((d.as_str(), t.as_str()), ("2026-06-27", "1805"));
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_at("K1ABC", "2A", "CT", 4, 1_782_583_500));
        let cab = log
            .cabrillo(14074)
            .expect("a single-mode event exports one entry");
        // Frequency is band-derived (20m → 14000 kHz), not the passed dial.
        assert!(
            cab.contains("QSO: 14000 DG 2026-06-27 1805 W9XYZ 3A WI K1ABC 2A CT"),
            "real date/time on the QSO line (ARRL submission requires it): {cab}"
        );
        assert!(!cab.contains("----------"), "no placeholder when stamped");
    }

    #[test]
    fn a_tempo_tier_contact_exports_as_a_mode_adif_actually_defines() {
        // ⚠️ THIS WAS A LOST CONTACT, not a formatting nit. The Field Day ADIF wrote the
        // recorded mode straight into <MODE>, and the tiers stamp names ADIF has never heard
        // of — TempoFast, TempoDeep, FT2. TQSL walks MODE%SUBMODE → SUBMODE → MODE and a bare
        // <MODE:10>TempoFast misses all three legs, so the record is DROPPED with "Invalid
        // MODE". The main logbook has solved this for years in `logbook::adif_submode`; the
        // Field Day exporter just never called it, so a Field Day contact worked on a Tempo
        // tier vanished on upload while an FT8 one beside it went through.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_submode_at("K1ABC", "2A", "CT", "DIG", "TempoFast", 4, 1_782_583_500));
        let adif = log.adif();
        assert!(
            adif.contains("<MODE:4>MFSK") && adif.contains("<SUBMODE:9>TEMPOFAST"),
            "a Tempo tier must ride as MODE=MFSK + SUBMODE, or TQSL drops the record: {adif}"
        );
        assert!(
            !adif.contains("<MODE:9>TempoFast"),
            "TempoFast is not an ADIF MODE and must never be written as one: {adif}"
        );

        // POSITIVE CONTROL: a mode ADIF really does define is written plainly, with no
        // SUBMODE invented for it — otherwise this test would pass against a change that
        // wrapped everything.
        let mut plain = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(plain.log_submode_at("K2DEF", "2A", "CT", "DIG", "RTTY", 4, 1_782_583_500));
        let plain_adif = plain.adif();
        assert!(
            plain_adif.contains("<MODE:4>RTTY"),
            "RTTY is a real MODE: {plain_adif}"
        );
        assert!(
            !plain_adif.contains("SUBMODE"),
            "…and needs no SUBMODE: {plain_adif}"
        );
    }

    #[test]
    fn cabrillo_says_the_band_the_contact_was_actually_made_on() {
        // ⚠️ TWO WAYS THIS LIED ABOUT A CONTACT'S BAND, both found by auditing our output
        // against the Cabrillo spec and Winter Field Day's parser.
        //
        // 1. ABOVE 50 MHz THE FIELD IS A BAND TOKEN, NOT KILOHERTZ. The spec's own words:
        //    "freq is frequency or band: 1800 or actual frequency in kHz […] 50 / 70 / 144 /
        //    222 / 432 / 902 / 1.2G". WFD states the same rule and MACHINE-PARSES the QSO
        //    lines out of whatever is uploaded, so a 6-digit value where a token belongs is a
        //    parse problem there. 70 cm was wrong twice over — the token is 432 and 420000 was
        //    not even a sensible kHz for it.
        // 2. AN UNRECOGNISED BAND TOOK THE CALLER'S FALLBACK, and the club exporter passed a
        //    HARDCODED 14 MHz. A 23 cm club contact exported as 20 m. ARRL Field Day does not
        //    read Cabrillo at all, but it DOES require a list of stations worked sorted BY
        //    BAND — so this corrupted the one artifact they actually use.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        for (band, call) in [
            ("20m", "K1ABC"),
            ("6m", "K2DEF"),
            ("2m", "K3GHI"),
            ("70cm", "K4JKL"),
            ("23cm", "K5MNO"),
            ("4m", "K6PQR"),
        ] {
            log.band = band.to_string();
            assert!(
                log.log_at(call, "2A", "CT", 4, 1_782_583_500),
                "{band} logged"
            );
        }
        let cab = log
            .cabrillo(14_000)
            .expect("a single-mode event exports one entry");

        // HF stays kHz.
        assert!(
            cab.contains("QSO: 14000 DG 2026-06-27 1805 W9XYZ 3A WI K1ABC"),
            "20m: {cab}"
        );
        // Above 50 MHz is the BAND, per the spec's own list.
        for (token, call) in [("50", "K2DEF"), ("144", "K3GHI"), ("432", "K4JKL")] {
            assert!(
                cab.contains(&format!(
                    "QSO: {token} DG 2026-06-27 1805 W9XYZ 3A WI {call}"
                )),
                "expected band token {token} for {call}: {cab}"
            );
        }
        // 4 m and 23 cm are legal Field Day bands and were not in the table at all — they
        // took the caller's fallback and claimed to be somewhere else entirely.
        assert!(
            cab.contains("QSO: 70 DG 2026-06-27 1805 W9XYZ 3A WI K6PQR"),
            "4m: {cab}"
        );
        assert!(
            cab.contains("QSO: 1.2G DG 2026-06-27 1805 W9XYZ 3A WI K5MNO"),
            "23cm: {cab}"
        );
        // THE PIN: nothing above 6 m may claim to be on 20 m.
        assert_eq!(
            cab.matches("QSO: 14000 ").count(),
            1,
            "only the 20 m contact may say 14000 — a band we cannot map must never borrow \
             another band's frequency: {cab}"
        );
    }

    #[test]
    fn adif_round_trip_restores_log_and_dupes() {
        // The FD contest log lives only in memory, so the exit flush's ADIF is
        // its sole durable copy — merging it back into a fresh log (a restart
        // mid-event) must restore the QSOs, the dupe index, the sections and
        // real timestamps (not the '----------' Cabrillo placeholder).
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1D", "EMA", "CW", 0, 1_782_583_560));
        assert!(log.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        let adif = log.adif();

        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        restored.merge_adif(&adif, 0);
        assert_eq!(restored.qso_count(), 3, "all three contacts restored");
        for (call, mode) in [("K1ABC", "DIG"), ("K2DEF", "CW"), ("N0GHI", "PH")] {
            assert!(
                restored.is_dupe_mode(call, mode),
                "{call} {mode} restored into the dupe index"
            );
        }
        assert_eq!(restored.sections(), 3, "sections survive the round-trip");
        assert_eq!(restored.qso_points(), 2 + 2 + 1, "DIG 2 + CW 2 + PH 1");
        let cab = restored
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        assert!(
            cab.contains("2026-06-27"),
            "restored rows keep their real timestamp: {cab}"
        );
        assert!(!cab.contains("----------"), "no placeholder after restore");
    }

    #[test]
    fn adif_and_cabrillo_carry_the_actual_digital_mode() {
        // An RTTY WFD QSO must never export MODE=FT8 (a banned mode there) —
        // the recorded actual mode wins; rows without one keep the legacy
        // FT8/DG fallback so old logs export unchanged.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2M", "EPA"),
            "20m",
        );
        log.set_event(FdEvent::WinterFd);
        assert!(log.log_submode_at("K1ABC", "1H", "CT", "DIG", " rtty ", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1O", "EMA", "DIG", 0, 1_782_583_560));
        let adif = log.adif();
        assert!(
            adif.contains("<MODE:4>RTTY"),
            "RTTY row exports its real (normalized) mode: {adif}"
        );
        assert!(adif.contains("<MODE:3>FT8"), "unrecorded row falls back");
        let cab = log
            .cabrillo(14_080)
            .expect("a single-mode event exports one entry");
        assert!(cab.contains(" RY "), "Cabrillo RY for the RTTY row: {cab}");
        assert!(cab.contains(" DG "), "DG fallback for the unrecorded row");
        // FD/WFD digital is ONE mode class: an RTTY try after the same-band
        // FT8 contact is a dupe, and both rows score the digital 2 points.
        assert!(
            logged_dupe(
                log.log_submode_at("K2DEF", "1O", "EMA", "DIG", "RTTY", 0, 1_782_583_620),
                &log
            ),
            "RTTY dupes the same-band digital contact"
        );
        assert_eq!(log.qso_points(), 4);
    }

    #[test]
    fn actual_mode_survives_the_adif_round_trip() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2M", "EPA"),
            "20m",
        );
        log.set_event(FdEvent::WinterFd);
        assert!(log.log_submode_at("K1ABC", "1H", "CT", "DIG", "RTTY", 0, 1_782_583_500));
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2M", "EPA"),
            "20m",
        );
        restored.set_event(FdEvent::WinterFd);
        restored.merge_adif(&log.adif(), 0);
        assert_eq!(restored.qso_count(), 1);
        let q = &restored.qsos()[0];
        assert_eq!((q.mode.as_str(), q.submode.as_str()), ("DIG", "RTTY"));
        assert!(
            restored.is_dupe_mode("K1ABC", "DIG"),
            "dupe key restored on the mode CLASS"
        );
        assert!(
            restored.adif().contains("<MODE:4>RTTY"),
            "re-export keeps RTTY"
        );
        assert!(restored
            .cabrillo(14_080)
            .expect("a single-mode event exports one entry")
            .contains(" RY "));
    }

    /// ⭐ **A FIELD DAY CONTACT KEEPS ITS DIAL ACROSS A RESTART** (operator ruling,
    /// 2026-09-20).
    ///
    /// Field Day's journal and its submitted export are the SAME function, and
    /// ⛔ **A DIGITAL SUBMODE SURVIVES A MID-EVENT RESTART** (operator review of 1.15.0,
    /// findings A3 and L2 — found independently by the adif and logbook lenses).
    ///
    /// The journal writes `<MODE:4>MFSK <SUBMODE:9>TEMPOFAST`; the reader adopted `SUBMODE`
    /// only when it was exactly `USB` or `LSB`, so the row came back as bare "MFSK". That
    /// reached the SUBMITTED Field Day file and the merged lifetime record, and an operator
    /// filtering "worked on TempoFast" — or a submode-sensitive award — never saw the
    /// contact. TempoDeep, FT4, FT2, Q65, FST4 and FST4W were lost the same way.
    ///
    /// ⚠️ RTTY is the control, and it is why the existing round-trip test could not see
    /// this: RTTY is a real ADIF MODE, so it never needed promoting and survived either way.
    #[test]
    fn a_digital_submode_survives_the_field_day_journal_round_trip() {
        for (submode, adif_mode) in [
            ("TempoFast", "MFSK"),
            ("TempoDeep", "MFSK"),
            ("FT4", "MFSK"),
            ("Q65", "MFSK"),
            ("FST4", "MFSK"),
        ] {
            let mut log = FieldDayLog::new(
                "W9XYZ",
                ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
                "20m",
            );
            assert!(log.log_submode_at("K1ABC", "1H", "CT", "DIG", submode, 0, 1_782_583_500));
            let journal = log.adif();
            // Precondition: the journal really does split it into parent + SUBMODE, or the
            // reader below would be restoring from something else entirely.
            assert!(
                journal.contains(&format!("<MODE:{}>{adif_mode}", adif_mode.len())),
                "{submode}: journal did not write the parent MODE: {journal}"
            );
            assert!(
                journal.to_ascii_uppercase().contains(&format!(
                    "<SUBMODE:{}>{}",
                    submode.len(),
                    submode.to_ascii_uppercase()
                )),
                "{submode}: journal did not write the SUBMODE: {journal}"
            );

            let mut restored = FieldDayLog::new(
                "W9XYZ",
                ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
                "20m",
            );
            restored.merge_adif(&journal, 0);
            assert_eq!(restored.qso_count(), 1, "{submode}");
            assert_eq!(
                restored.qsos()[0].recorded_mode().to_ascii_uppercase(),
                submode.to_ascii_uppercase(),
                "{submode} came back as something else after a restart"
            );
        }

        // THE CONTROL. RTTY is a real ADIF MODE, so it round-trips with no promotion at all
        // — it passed before this fix and must still pass, which is what says the change is
        // an ADDITION to the reader rather than a different reader.
        let mut rtty = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
            "20m",
        );
        assert!(rtty.log_submode_at("K2DEF", "1H", "CT", "DIG", "RTTY", 0, 1_782_583_500));
        let mut back = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
            "20m",
        );
        back.merge_adif(&rtty.adif(), 0);
        assert_eq!(back.qsos()[0].recorded_mode(), "RTTY", "the control moved");
    }

    /// [`Self::merge_adif`] restores `freq_khz` by reading `FREQ` back out of it
    /// (`self.qsos` ← `f.get("FREQ")`). So while `adif` withheld `FREQ` for Field Day
    /// alone, **a restart mid-event silently zeroed the dial on every contact already
    /// logged**, and nothing told the operator.
    ///
    /// ⚠️ **The exception's written justification was FALSE, and was measured before it
    /// was removed.** It read "its journal and export are pinned byte for byte by the
    /// §8(a) goldens" — but every row in the golden fixtures carries `dial_khz == 0`, so
    /// all ten pass with the clause or without it. The goldens never held it in place.
    /// A justification nobody can reproduce is how an exception outlives its reason.
    #[test]
    fn a_field_day_contact_keeps_its_dial_across_a_restart() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
            "20m",
        );
        log.dial_khz = 14_253;
        assert!(log.log_mode_at("K1ABC", "1H", "CT", "PH", 0, 1_782_583_500));
        assert_eq!(
            log.qsos()[0].freq_khz,
            14_253,
            "precondition: the row is stamped with the dial at log time"
        );

        // The journal must carry it, or the reader below has nothing to restore from.
        assert!(
            log.adif().contains("<FREQ:9>14.253000"),
            "Field Day's journal dropped the dial: {}",
            log.adif()
        );
        // ⭐ AND THE DIAL ITSELF, in its own tag. `FREQ` is the on-air frequency now, so it
        // is no longer the thing `merge_adif` may rebuild the Cabrillo kHz from.
        assert!(
            log.adif().contains("<APP_NEXUS_DIALKHZ:5>14253"),
            "the Cabrillo dial is not journalled: {}",
            log.adif()
        );

        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
            "20m",
        );
        restored.merge_adif(&log.adif(), 0);
        assert_eq!(restored.qso_count(), 1);
        assert_eq!(
            restored.qsos()[0].freq_khz,
            14_253,
            "a restart zeroed the dial on a contact that was logged with one"
        );
    }

    /// ⭐ **A PHONE CONTEST CONTACT CARRIES THE MODE IT WAS WORKED ON, THROUGH BOTH
    /// EXPORTS AND THROUGH THE JOURNAL** (operator review of 1.14.0).
    ///
    /// `PH` is a SCORING CLASS, not a mode — one point either event, and it covers SSB,
    /// AM and FM alike. The general log has recorded the actual phone mode since
    /// 2026-09-15 (`logbook::adif_submode`: `<MODE:3>SSB` + `<SUBMODE:3>USB`, the ADIF
    /// 3.0.4/3.1.7 pairing), and the contest path was left standing: every phone row
    /// exported as bare `SSB`, so no contest contact said upper or lower and an FM
    /// contact claimed an emission that was never on the air.
    ///
    /// ⚠️ **The half that had to move HERE is the journal READER.** [`Self::adif`] already
    /// wrote the recorded mode through `adif_submode`; `restore_row` inverted `MODE`
    /// alone, so a restart dropped the sideband — and filed `<MODE:2>FM` as a `DIG` row,
    /// which is the wrong mode class, the wrong dupe bucket and TWO points instead of one.
    #[test]
    fn a_phone_contact_carries_its_on_air_mode_through_the_exports_and_the_journal() {
        let fresh = || {
            FieldDayLog::new(
                "W9XYZ",
                ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
                "40m",
            )
        };
        for (submode, golden) in [
            ("LSB", "<MODE:3>SSB <SUBMODE:3>LSB "),
            ("USB", "<MODE:3>SSB <SUBMODE:3>USB "),
            ("FM", "<MODE:2>FM "),
            ("AM", "<MODE:2>AM "),
        ] {
            let mut log = fresh();
            assert!(log.log_submode_at("K1ABC", "2A", "EMA", "PH", submode, 0, 1_782_583_500));
            let adif = log.adif();
            assert!(
                adif.contains(golden),
                "{submode} must ride as {golden}: {adif}"
            );
            // ⛔ The bare sideband must never reach MODE — that is the record TQSL drops
            // on the closed Mode enumeration, exactly as a bare `<MODE:9>TempoFast` is.
            assert!(
                !adif.contains("<MODE:3>USB") && !adif.contains("<MODE:3>LSB"),
                "{submode} emitted the bare invalid mode: {adif}"
            );
            // The SCORING CLASS is untouched: phone is one point whatever the emission,
            // and the Cabrillo QSO line writes the class, never the mode.
            assert_eq!(
                log.qso_points(),
                1,
                "{submode} is still a one-point phone QSO"
            );
            let cab = log
                .cabrillo(7_074)
                .expect("a single-mode event exports one entry");
            assert!(cab.contains(" PH "), "{submode} keeps the PH token: {cab}");

            // …and a restart restores the SAME row — class, mode and points.
            let mut back = fresh();
            back.merge_adif(&adif, 0);
            assert_eq!(back.qso_count(), 1, "{submode} row lost on restore");
            let q = &back.qsos()[0];
            assert_eq!(
                (q.mode.as_str(), q.submode.as_str()),
                ("PH", submode),
                "{submode}: the journal round trip lost the phone mode"
            );
            assert_eq!(
                back.qso_points(),
                1,
                "{submode}: a restored phone row is one point"
            );
            assert!(
                back.adif().contains(golden),
                "{submode}: re-export after a restart lost it: {}",
                back.adif()
            );
        }

        // POSITIVE CONTROL, and it is what keeps the round trip above from passing under a
        // reader that answers `("PH", <whatever MODE said>)` for everything. A row with NO
        // recorded phone mode — every row logged before this existed — keeps the legacy
        // class map exactly: bare `SSB`, no `SUBMODE`, and it restores with an EMPTY
        // submode rather than acquiring "SSB" as though the sideband had been observed.
        let mut legacy = fresh();
        assert!(legacy.log_mode_at("W1AW", "1D", "CT", "PH", 0, 1_782_583_560));
        let adif = legacy.adif();
        assert!(adif.contains("<MODE:3>SSB "), "{adif}");
        assert!(
            !adif.contains("<SUBMODE:"),
            "no sideband was observed: {adif}"
        );
        let mut back = fresh();
        back.merge_adif(&adif, 0);
        assert_eq!(
            (
                back.qsos()[0].mode.as_str(),
                back.qsos()[0].submode.as_str()
            ),
            ("PH", ""),
            "a legacy phone row must not invent evidence it never had"
        );
        // …and CW and digital are not phone and must not be dragged through the phone arm.
        let mut other = fresh();
        assert!(other.log_mode_at("K2DEF", "1D", "EMA", "CW", 0, 1_782_583_620));
        assert!(other.log_submode_at("K3GHI", "1D", "EMA", "DIG", "RTTY", 0, 1_782_583_680));
        let mut back = fresh();
        back.merge_adif(&other.adif(), 0);
        assert_eq!(
            back.qsos()
                .iter()
                .map(|q| (q.mode.as_str(), q.submode.as_str()))
                .collect::<Vec<_>>(),
            vec![("CW", ""), ("DIG", "RTTY")],
            "the phone arm must not swallow CW or a digital row"
        );
    }

    /// ⭐ **Cabrillo's `CATEGORY-MODE` has an `FM` token, and an FM log must use it.**
    ///
    /// The header is derived from the rows' own modes, and it read the scoring CLASS: `PH`
    /// answered `SSB` for every phone row, so an FM-only log declared an SSB entry. The
    /// Cabrillo V3 enumeration is `CW / DIGI / FM / RTTY / SSB / MIXED` — there is no `AM`
    /// token, so AM rides as the phone token `SSB`, which is the closest legal claim.
    ///
    /// ⚠️ **Asserted on the function rather than on an exported file, because the header is
    /// not reachable from one today** (checked 2026-09-20): `cabrillo_with` writes it only
    /// for a ruleset whose `cabrillo.headers` lists it, and of the seventeen shipped
    /// rulesets exactly one does — `cqww_rtty`, which is RTTY-only. The three ARRL VHF
    /// rulesets, where an FM entry is ordinary, declare no headers at all. So this fixes
    /// the derivation before the first ruleset that could print it lands.
    #[test]
    fn the_cabrillo_category_mode_names_fm_rather_than_calling_it_ssb() {
        let rows = |modes: &[(&str, &str)]| -> String {
            let mut log = FieldDayLog::new(
                "W9XYZ",
                ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
                "2m",
            );
            for (i, (mode, submode)) in modes.iter().enumerate() {
                assert!(log.log_submode_at(
                    &format!("K{i}ABC"),
                    "2A",
                    "EMA",
                    mode,
                    submode,
                    0,
                    1_782_583_500 + i as u64,
                ));
            }
            cabrillo_category_mode(log.qsos())
        };
        assert_eq!(rows(&[("PH", "FM"), ("PH", "FM")]), "FM", "an FM-only log");
        assert_eq!(
            rows(&[("PH", "USB"), ("PH", "LSB")]),
            "SSB",
            "both sidebands are SSB"
        );
        assert_eq!(
            rows(&[("PH", "")]),
            "SSB",
            "a row with no recorded mode is SSB"
        );
        assert_eq!(rows(&[("PH", "AM")]), "SSB", "Cabrillo has no AM token");
        // The discriminating case, and the reason this is not one assertion: FM and SSB in
        // one log is MIXED, which is only visible once the two stop being the same answer.
        assert_eq!(rows(&[("PH", "FM"), ("PH", "USB")]), "MIXED");
        // …and the arms that already worked still do.
        assert_eq!(rows(&[("CW", "")]), "CW");
        assert_eq!(rows(&[("DIG", "RTTY")]), "RTTY");
        assert_eq!(rows(&[("DIG", "FT8")]), "DIGI");
        assert_eq!(rows(&[("CW", ""), ("PH", "USB")]), "MIXED");
        assert_eq!(
            cabrillo_category_mode(&[]),
            "",
            "an empty log writes no header"
        );
    }

    #[test]
    fn seq_is_stamped_monotonic_and_round_trips_as_app_nexus_qseq() {
        // Every entry point funnels into log_submode_at, so seqs are 1..n in
        // log order regardless of mode/path.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1D", "EMA", "CW", 0, 1_782_583_560));
        assert_eq!(log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(log.max_seq(), 2);
        let adif = log.adif();
        assert!(
            adif.contains("<APP_NEXUS_QSEQ:1>1") && adif.contains("<APP_NEXUS_QSEQ:1>2"),
            "seq journaled per row: {adif}"
        );

        // Restore keeps each row's seq and the NEXT log entry continues past
        // the restored high-water — the property that makes a restart safe
        // against re-issuing an id the club host already merged.
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        restored.merge_adif(&adif, 0);
        assert_eq!(
            restored.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [1, 2],
            "seqs survive the round-trip"
        );
        assert!(restored.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        assert_eq!(
            restored.qsos()[2].seq,
            3,
            "fresh rows continue the sequence"
        );
    }

    #[test]
    fn legacy_journal_without_qseq_backfills_in_row_order() {
        // A pre-sync journal has no APP_NEXUS_QSEQ tags at all: restore assigns
        // 1..n in row order (deterministic, so a re-restore re-derives the same
        // ids) and new contacts continue from there.
        let legacy = "<EOH>\n\
            <CALL:5>K1ABC <MODE:3>FT8 <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180500 \
            <CLASS:2>2A <ARRL_SECT:2>CT <EOR>\n\
            <CALL:5>K2DEF <MODE:2>CW <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180600 \
            <CLASS:2>1D <ARRL_SECT:3>EMA <EOR>\n";
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.merge_adif(legacy, 0);
        assert_eq!(
            log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [1, 2],
            "legacy rows backfill 1..n in row order"
        );
        assert!(log.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        assert_eq!(log.qsos()[2].seq, 3);
        // A mixed journal (one stamped row, one legacy) never collides: the
        // backfill takes the next free seq at the point the row restores.
        let mixed = "<EOH>\n\
            <CALL:5>K1ABC <MODE:3>FT8 <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180500 \
            <CLASS:2>2A <ARRL_SECT:2>CT <APP_NEXUS_QSEQ:1>5 <EOR>\n\
            <CALL:5>K2DEF <MODE:2>CW <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180600 \
            <CLASS:2>1D <ARRL_SECT:3>EMA <EOR>\n";
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.merge_adif(mixed, 0);
        assert_eq!(
            log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [5, 6],
            "backfill continues past a restored explicit seq"
        );
        assert_eq!(log.max_seq(), 6);
    }

    #[test]
    fn merge_adif_age_gate_and_garbage_merge_nothing() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        let adif = log.adif();

        // A min_when_unix newer than every row (a previous event's journal)
        // restores nothing — the backup self-expires.
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        restored.merge_adif(&adif, 1_782_583_501);
        assert_eq!(restored.qso_count(), 0, "expired journal merges nothing");

        // Garbage input merges nothing and never errors.
        restored.merge_adif("not adif <CALL:junk><EOR> \u{fe0f}<QSO_DATE:8>x<EOR>", 0);
        assert_eq!(restored.qso_count(), 0, "garbage merges nothing");
    }

    /// What a merge reports: the rows it restored, and the records it could not read (no CALL,
    /// or cut off before their `<EOR>`), which the journal's reader keeps the file for. Rows it
    /// passes over on purpose, a previous event's or ones already in the log, are neither.
    #[test]
    fn merge_adif_reports_the_records_it_could_not_read() {
        let fresh = || {
            FieldDayLog::new(
                "W9XYZ",
                ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
                "20m",
            )
        };
        let mut log = fresh();
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        assert!(log.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        let whole = log.adif();
        let merged = |text: &str, min_when_unix: u64| fresh().merge_adif(text, min_when_unix);
        assert_eq!(
            merged(&whole, 0),
            AdifMerge {
                restored: 2,
                unreadable: 0
            },
            "a whole journal"
        );
        let last_eor = whole.rfind("<EOR>").expect("a record end");
        assert_eq!(
            merged(&whole[..last_eor - 4], 0),
            AdifMerge {
                restored: 1,
                unreadable: 1
            },
            "cut off inside its last record"
        );
        let no_call = whole.replacen("<CALL:5>K1ABC", "<NOTE:5>K1ABC", 1);
        assert_ne!(no_call, whole, "the fixture must lose a call");
        assert_eq!(
            merged(&no_call, 0),
            AdifMerge {
                restored: 1,
                unreadable: 1
            },
            "a record with no call"
        );
        assert_eq!(
            merged(&whole, 1_782_583_501),
            AdifMerge {
                restored: 1,
                unreadable: 0
            },
            "a previous event's row is passed over, not unreadable"
        );
        let mut again = fresh();
        again.merge_adif(&whole, 0);
        assert_eq!(
            again.merge_adif(&whole, 0),
            AdifMerge::default(),
            "rows already in the log are passed over, not unreadable"
        );
    }

    use super::*;
    use crate::contest::ContestSession;

    fn dec(msg: &str) -> Decode {
        Decode {
            message: msg.to_string(),
            sync: 1.0,
            snr: -5,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            raw: None,
            mode: None,
        }
    }

    /// The write `logged` answered for entered the log and is marked a dupe: what every
    /// contest does with one since the operator's ruling of 2026-10-11.
    fn logged_dupe(logged: bool, log: &FieldDayLog) -> bool {
        logged && log.qsos().last().is_some_and(|q| q.dupe)
    }

    #[test]
    fn dupe_check_and_scoring() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        assert!(log.log("K2DEF", "3A", "IL", 1));
        assert!(log.log("N0ABC", "1D", "MN", 2));
        assert!(logged_dupe(log.log("K2DEF", "3A", "IL", 3), &log)); // dupe on same band
        assert_eq!(log.qso_count(), 2);
        assert_eq!(log.sections(), 2); // IL, MN
        assert_eq!(log.qso_points(), 4); // 2 pts each
        assert!(log.adif().contains("ARRL_SECT") && log.adif().contains("K2DEF"));
        let cab = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        // The dupe is a plain QSO line too, worth nothing.
        assert_eq!(cab.matches("QSO:").count(), 3);
        assert!(cab.contains("W9XYZ 3A WI K2DEF 3A IL"));
    }

    #[test]
    fn cabrillo_frequency_is_per_qso_band_not_the_export_dial() {
        // Each row's frequency comes from the QSO's own band, not the single dial
        // the export happened to pass (which stamped every row before the fix).
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_at("K1ABC", "2A", "CT", 1, 1_782_583_500)); // 20m
        let cab = log
            .cabrillo(99999)
            .expect("a single-mode event exports one entry"); // dial fallback must NOT appear for a known band
        assert!(
            cab.contains("QSO: 14000 "),
            "20m QSO stamped 14000 kHz: {cab}"
        );
        assert!(
            !cab.contains("QSO: 99999 "),
            "the export dial is not stamped on a known band"
        );
    }

    #[test]
    fn worked_sections_returns_the_distinct_set_sorted() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        assert!(log.log("K2DEF", "3A", "IL", 1));
        assert!(log.log("N0ABC", "1D", "MN", 2));
        assert!(log.log_mode_at("W1AW", "2A", "IL", "CW", 0, 100)); // IL again, new mode
                                                                    // The count and the identities agree; identities are the distinct set,
                                                                    // sorted (IL once, though worked twice), and stable.
        assert_eq!(log.worked_sections(), vec!["IL", "MN"]);
        assert_eq!(log.worked_sections().len(), log.sections());
    }

    #[test]
    fn observe_runs_sp_side() {
        // S&P station hears a CQ, sends exchange, then the runner's rogered exch.
        let mut sp = FieldDayStation::search_and_pounce(
            "K2DEF",
            "FN31",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "IL"),
            "20M",
        );
        sp.observe(&[dec("CQ W9XYZ EN37")], 0);
        assert_eq!(sp.state, FdState::Listening, "S&P answers no CQ by itself");
        sp.answer_cq("W9XYZ", "EN37"); // the operator's double-click
        assert_eq!(sp.state, FdState::AwaitExchange);
        assert_eq!(sp.outgoing().unwrap().to_text(), "W9XYZ K2DEF 2A IL");
        sp.observe(&[dec("K2DEF W9XYZ R 3A WI")], 1);
        assert_eq!(sp.state, FdState::Done);
        assert_eq!(sp.log.qso_count(), 1);
        assert_eq!(sp.log.qsos()[0].class(), "3A");
        assert_eq!(sp.log.qsos()[0].section(), "WI");
    }

    #[test]
    fn running_station_calls_directed_cq_fd() {
        // The running station advertises a DIRECTED Field Day CQ so it reads as
        // "CQ FD" on the air — and an S&P station still answers it.
        let run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        let cq = run.outgoing().unwrap().to_text();
        assert!(cq.contains("CQ FD"), "directed FD CQ: {cq}");
        assert_eq!(cq, "CQ FD W9XYZ EN37");
        let mut sp = FieldDayStation::search_and_pounce(
            "K2DEF",
            "FN31",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "IL"),
            "20M",
        );
        let Msg::Cq { de, grid, .. } = Msg::parse(&cq) else {
            panic!("a CQ: {cq}");
        };
        sp.answer_cq(&de, &grid);
        assert_eq!(
            sp.state,
            FdState::AwaitExchange,
            "a double-click answers a directed FD CQ"
        );
    }

    #[test]
    fn running_station_rearms_and_works_a_second_caller() {
        // Regression for the RUN dead-end: a running station worked exactly one
        // contact and then went silent (Done, no return to CQ). It must re-arm.
        let mut run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0); // caller's exchange
        assert_eq!(run.state, FdState::AwaitConfirm);
        run.observe(&[dec("W9XYZ K2DEF RR73")], 1); // caller confirms → we log
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        assert!(run.done(), "closed after the first QSO");

        // The fix: the engine re-arms a `done()` RUN station back to calling CQ.
        run.rearm(true);
        assert_eq!(run.state, FdState::CallingCq);
        assert!(
            run.outgoing().unwrap().to_text().contains("CQ FD"),
            "back on CQ FD after re-arm"
        );

        // It now works a SECOND caller, and the first is still a dupe (the
        // in-station contest log survived the re-arm).
        run.observe(&[dec("W9XYZ N0ABC 1D MN")], 2);
        assert_eq!(run.state, FdState::AwaitConfirm, "answers the next caller");
        run.observe(&[dec("W9XYZ N0ABC RR73")], 3);
        assert_eq!(run.log.qso_count(), 2, "second contact logs after re-arm");
        assert!(run.log.is_dupe("K2DEF"), "first caller remains a dupe");
    }

    #[test]
    fn loopback_completes_the_exchange_on_the_happy_path() {
        // The full round-trip over the real modem still completes unchanged after
        // the directed-CQ-FD switch: both stations log the OTHER's exchange.
        let mut running = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        let mut sp = FieldDayStation::search_and_pounce(
            "K2DEF",
            "FN31",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "IL"),
            "20M",
        );
        run_loopback_fieldday(&mut running, &mut sp, 15.0, 40);
        assert_eq!(
            running.log.qso_count(),
            1,
            "running: {:?}",
            running.transcript
        );
        assert_eq!(sp.log.qso_count(), 1, "sp: {:?}", sp.transcript);
        assert_eq!(
            running.log.qsos()[0].section(),
            "IL",
            "running logged the caller"
        );
        assert_eq!(sp.log.qsos()[0].section(), "WI", "S&P logged the runner");
    }

    #[test]
    fn running_logs_on_a_bare_73() {
        // Caller closes with a bare `73` instead of RR73/RRR: the contact must
        // still complete + log (the caller's exchange was captured at CallingCq),
        // not stall in AwaitConfirm.
        let mut run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0); // caller's exchange
        assert_eq!(run.state, FdState::AwaitConfirm);
        run.observe(&[dec("W9XYZ K2DEF 73")], 1); // bare 73, not RR73
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        let q = &run.log.qsos()[0];
        assert_eq!(
            (q.call.as_str(), q.class(), q.section()),
            ("K2DEF", "2A", "IL")
        );
    }

    #[test]
    fn running_skips_ahead_on_an_early_rogered_exchange() {
        // A caller that ROGERS its exchange while we're still calling CQ (the
        // plain exchange dropped, or the caller pre-rogered): the class + section
        // are in the rogered frame, so skip straight to logging + close with RR73
        // instead of stalling on CQ.
        let mut run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        assert_eq!(run.state, FdState::CallingCq);
        run.observe(&[dec("W9XYZ K2DEF R 2A IL")], 0); // rogered, skipping the plain form
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        let q = &run.log.qsos()[0];
        assert_eq!(
            (q.call.as_str(), q.class(), q.section()),
            ("K2DEF", "2A", "IL")
        );
        assert_eq!(
            run.outgoing().unwrap().to_text(),
            "K2DEF W9XYZ RR73",
            "closes with RR73 to the caller"
        );
    }

    // -- each contest's own FT8/FT4 messages ------------------------------------

    /// A station in the rules file's `event`, with `mygrid` in Settings, speaking what the
    /// engine picks for that contest on FT8.
    fn ft8_station(event: &str, mygrid: &str, running: bool) -> FieldDayStation {
        ft8_station_as("K1ABC", event, mygrid, running)
    }

    /// [`ft8_station`] for the station `mycall`.
    fn ft8_station_as(mycall: &str, event: &str, mygrid: &str, running: bool) -> FieldDayStation {
        let rs = crate::fd_rules::ruleset_by_id(event, crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let data = crate::contest::StationData {
            mycall: mycall.into(),
            mygrid: mygrid.into(),
            contest_qth_state: "MA".into(),
            ..Default::default()
        };
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        let mut st = if running {
            FieldDayStation::running(mycall, mygrid, session, "6M")
        } else {
            FieldDayStation::search_and_pounce(mycall, mygrid, session, "6M")
        };
        st.set_messages(FtMessages::for_contest(rs, "FT8"), running);
        st
    }

    /// What the station sends next, as it goes on the air.
    fn next(st: &FieldDayStation) -> String {
        st.outgoing().map(|m| m.to_text()).unwrap_or_default()
    }

    /// The operator's double-click on the CQ `cq` ([`FieldDayStation::answer_cq`]): S&P answers
    /// no CQ by itself, so this is how every S&P contact starts.
    fn click(st: &mut FieldDayStation, cq: &str) {
        let Msg::Cq { de, grid, .. } = Msg::parse(cq) else {
            panic!("a CQ: {cq}");
        };
        st.answer_cq(&de, &grid);
    }

    /// The newest row: call, received fields, sent fields.
    fn last_row(st: &FieldDayStation) -> (String, Vec<String>, Vec<String>) {
        let q = st.log.qsos().last().expect("a row");
        let kv = |v: &[crate::contest::FieldValue]| -> Vec<String> {
            v.iter().map(|f| format!("{}={}", f.key, f.raw)).collect()
        };
        (q.call.clone(), kv(&q.rx), kv(&q.tx))
    }

    /// ⭐ Which messages every shipped contest speaks, on every tier — and the table is the
    /// whole rules file, so a contest added to it is a failure here until someone decides.
    #[test]
    fn each_contest_speaks_the_wsjtx_activity_it_matches() {
        use FtMessages::*;
        let year = crate::fd_rules::CURRENT_RULES_YEAR;
        let want = [
            ("arrlfd", FieldDay),
            // Winter Field Day, Illinois and New York ban FT8 and FT4: nothing new.
            ("wfd", FieldDay),
            ("ilqp", FieldDay),
            ("nyqp", FieldDay),
            ("arrlvhf_jan", NaVhf),
            ("arrlvhf_jun", NaVhf),
            ("arrlvhf_sep", NaVhf),
            ("arrlss_cw", Standard),
            ("arrlss_ssb", Standard),
            ("cqww_cw", Standard),
            ("cqww_ssb", Standard),
            ("cqww_rtty", Standard),
            ("cqwpx_cw", Standard),
            ("cqwpx_ssb", Standard),
            ("cqp", Standard),
            ("ohqp", Standard),
            ("tnqp", Standard),
            ("txqp", Standard),
        ];
        for (event, messages) in want {
            let rs = crate::fd_rules::ruleset_by_id(event, year).expect(event);
            for mode in ["FT8", "FT4"] {
                assert_eq!(
                    FtMessages::for_contest(rs, mode),
                    messages,
                    "{event} on {mode}"
                );
            }
            for mode in ["FT2", "TempoFast", "TempoDeep", "RTTY", ""] {
                assert_eq!(
                    FtMessages::for_contest(rs, mode),
                    FieldDay,
                    "{event} on {mode:?} keeps Field Day's messages"
                );
            }
        }
        let seed: serde_json::Value =
            serde_json::from_str(include_str!("fd_rules.seed.json")).expect("the seed parses");
        let mut shipped: Vec<&str> = seed["rulesets"]
            .as_array()
            .expect("rulesets")
            .iter()
            .filter_map(|r| r["event"].as_str())
            .collect();
        let mut listed: Vec<&str> = want.iter().map(|(e, _)| *e).collect();
        shipped.sort_unstable();
        listed.sort_unstable();
        assert_eq!(listed, shipped, "every shipped contest is decided above");
    }

    /// ⭐ The activity each contest's own stations run, whatever the tier and whatever its rules
    /// ban, and the number WSJT-X reports for it in its UDP Status: Field Day 3 in ARRL Field
    /// Day only, NA VHF 1 in the three ARRL VHF contests, NONE 0 everywhere else — Winter Field
    /// Day included, whose categories WSJT-X's Field Day exchange does not take.
    #[test]
    fn each_contest_reports_the_activity_wsjtx_would() {
        let year = crate::fd_rules::CURRENT_RULES_YEAR;
        let seed: serde_json::Value =
            serde_json::from_str(include_str!("fd_rules.seed.json")).expect("the seed parses");
        let mut events: Vec<&str> = seed["rulesets"]
            .as_array()
            .expect("rulesets")
            .iter()
            .filter_map(|r| r["event"].as_str())
            .collect();
        events.push("");
        assert_eq!(events.len(), 19);
        for event in events {
            let want = match event {
                "" | "arrlfd" => (FtMessages::FieldDay, 3),
                "arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep" => (FtMessages::NaVhf, 1),
                _ => (FtMessages::Standard, 0),
            };
            let got = FtMessages::of_contest(event);
            assert_eq!((got, got.special_op()), want, "{event:?}");
            if let Some(rs) = crate::fd_rules::ruleset_by_id(event, year) {
                let banned = rs
                    .banned_modes
                    .iter()
                    .any(|b| b.eq_ignore_ascii_case("FT8"));
                if !banned {
                    assert_eq!(FtMessages::for_contest(rs, "FT8"), got, "{event} on FT8");
                }
            }
        }
        assert_eq!(FtMessages::of_contest("wfd"), FtMessages::Standard);
    }

    /// ⛔ S&P never starts a contact by itself, in each contest's messages: a CQ it hears is passed
    /// by with nothing queued, as WSJT-X never calls a station the operator did not pick. The
    /// operator's double-click on that CQ (or a logger's Reply to it) starts the contact, which
    /// runs to the log as before; then S&P listens again, and the next CQ it hears is passed by
    /// too.
    #[test]
    fn sp_starts_no_contact_by_itself_and_a_double_click_runs_to_the_log() {
        let sp = |event: &str| {
            if event == "arrlfd" {
                FieldDayStation::search_and_pounce(
                    "K1ABC",
                    "FN42",
                    ContestSession::field_day(FdEvent::ArrlFd, "2B", "EMA"),
                    "20M",
                )
            } else {
                ft8_station(event, "FN42", false)
            }
        };
        let row = |rx: &[&str], tx: &[&str]| {
            let v = |s: &[&str]| s.iter().map(|f| f.to_string()).collect::<Vec<_>>();
            ("W9XYZ".to_string(), v(rx), v(tx))
        };
        // Each contest's CQ, then each over this station sends with the reply that follows it,
        // the row it logs, and the next station's CQ.
        for (event, cq, overs, row, next_cq) in [
            (
                "arrlfd",
                "CQ FD W9XYZ EN37",
                &[
                    ("W9XYZ K1ABC 2B EMA", "K1ABC W9XYZ R 1D WI"),
                    ("W9XYZ K1ABC RR73", ""),
                ][..],
                row(&["CLASS=1D", "SECTION=WI"], &["CLASS=2B", "SECTION=EMA"]),
                "CQ FD K2DEF FN31",
            ),
            (
                "arrlvhf_jan",
                "CQ TEST W9XYZ EN37",
                &[
                    ("W9XYZ K1ABC FN42", "K1ABC W9XYZ R EN37"),
                    ("W9XYZ K1ABC RR73", ""),
                ][..],
                row(&["GRID=EN37"], &["GRID=FN42"]),
                "CQ TEST K2DEF FN31",
            ),
            (
                "txqp",
                "CQ W9XYZ EN37",
                &[
                    ("W9XYZ K1ABC FN42", "K1ABC W9XYZ -07"),
                    ("W9XYZ K1ABC R-05", "K1ABC W9XYZ RR73"),
                    ("W9XYZ K1ABC 73", ""),
                ][..],
                row(&[], &[]),
                "CQ K2DEF FN31",
            ),
        ] {
            let mut st = sp(event);
            st.observe(&[dec(cq)], 0);
            assert_eq!(
                (st.state, next(&st), st.dxcall.clone()),
                (FdState::Listening, String::new(), None),
                "{event}: a CQ heard is not answered"
            );
            st.answer_cq("W9XYZ", "EN37");
            let mut sent = Vec::new();
            for (slot, (_, reply)) in overs.iter().enumerate() {
                sent.push(next(&st));
                st.after_tx();
                if !reply.is_empty() {
                    st.observe(&[dec(reply)], slot as u64 + 1);
                }
            }
            let want: Vec<String> = overs.iter().map(|(o, _)| o.to_string()).collect();
            assert_eq!(sent, want, "{event}: the double-clicked contact");
            assert!(st.done(), "{event}: finished");
            assert_eq!(st.log.qso_count(), 1, "{event}: logged");
            assert_eq!(last_row(&st), row, "{event}: the row");
            st.rearm(false);
            st.observe(&[dec(next_cq)], 9);
            assert_eq!(
                (st.state, next(&st), st.dxcall.clone()),
                (FdState::Listening, String::new(), None),
                "{event}: the next CQ after the contact is not answered"
            );
        }
    }

    /// A station already in the log is answered all the same when the operator double-clicks its
    /// CQ: the operator picked it. Heard, its CQ is passed by, as every CQ is.
    #[test]
    fn a_double_click_answers_a_station_already_in_the_log() {
        let sp = || {
            FieldDayStation::search_and_pounce(
                "K1ABC",
                "FN42",
                ContestSession::field_day(FdEvent::ArrlFd, "2B", "EMA"),
                "20M",
            )
        };
        let (mut heard, mut clicked) = (sp(), sp());
        for st in [&mut heard, &mut clicked] {
            assert!(st.log.log("W9XYZ", "1D", "WI", 0));
        }
        heard.observe(&[dec("CQ FD W9XYZ EN37")], 1);
        clicked.answer_cq("W9XYZ", "EN37");
        assert_eq!(next(&heard), "", "S&P passes the CQ by");
        assert_eq!(
            next(&clicked),
            "W9XYZ K1ABC 2B EMA",
            "the double-click works it"
        );
    }

    /// A contact the operator picked (a double-click on its CQ) is told apart through its last
    /// over, and no longer: re-armed for the next, the station has picked nothing, and a CQ it
    /// then hears starts nothing. A contact the run answered is not picked.
    #[test]
    fn a_picked_contact_is_told_apart_until_the_station_rearms() {
        let session = || ContestSession::field_day(FdEvent::ArrlFd, "2B", "EMA");
        let mut clicked = FieldDayStation::search_and_pounce("K1ABC", "FN42", session(), "20M");
        clicked.answer_cq("W9XYZ", "EN37");
        clicked.after_tx();
        clicked.observe(&[dec("K1ABC W9XYZ R 1D WI")], 1);
        clicked.after_tx();
        assert_eq!(
            (clicked.done(), clicked.picked()),
            (true, true),
            "picked through its last over"
        );
        clicked.rearm(false);
        assert!(!clicked.picked(), "re-armed, nothing picked");
        clicked.observe(&[dec("CQ FD K2DEF FN31")], 2);
        assert_eq!(
            (clicked.state, clicked.picked(), next(&clicked)),
            (FdState::Listening, false, String::new()),
            "a CQ heard starts nothing"
        );
        let mut run = FieldDayStation::running("K1ABC", "FN42", session(), "20M");
        run.observe(&[dec("K1ABC W9XYZ 1D WI")], 1);
        assert_eq!(
            (run.state, run.picked()),
            (FdState::AwaitConfirm, false),
            "the run's caller"
        );
    }

    /// Which contests' FT8/FT4 stations follow the contest's own messages, and where the caller
    /// runs WSJT-X's Field Day activity, over the whole rules file: FT8 and FT4 follow every
    /// contest whose rules do not ban them (this test's own list of those that do), WSJT-X's
    /// Field Day is ARRL Field Day's alone, and no other tier follows either.
    #[test]
    fn who_follows_the_contest_and_who_runs_wsjtx_field_day() {
        let year = crate::fd_rules::CURRENT_RULES_YEAR;
        let seed: serde_json::Value =
            serde_json::from_str(include_str!("fd_rules.seed.json")).expect("the seed parses");
        let events: Vec<&str> = seed["rulesets"]
            .as_array()
            .expect("rulesets")
            .iter()
            .filter_map(|r| r["event"].as_str())
            .collect();
        assert_eq!(events.len(), 18);
        for event in events {
            let rs = crate::fd_rules::ruleset_by_id(event, year).expect(event);
            let bans = matches!(event, "wfd" | "ilqp" | "nyqp");
            for mode in ["FT8", "FT4"] {
                assert_eq!(
                    FtMessages::follows_contest(rs, mode),
                    !bans,
                    "{event} on {mode}"
                );
                assert_eq!(
                    FtMessages::wsjtx_field_day(rs, mode),
                    event == "arrlfd",
                    "{event} on {mode}"
                );
            }
            for mode in ["FT2", "TempoFast", "TempoDeep", "RTTY", ""] {
                assert!(
                    !FtMessages::follows_contest(rs, mode),
                    "{event} on {mode:?}"
                );
                assert!(
                    !FtMessages::wsjtx_field_day(rs, mode),
                    "{event} on {mode:?}"
                );
            }
        }
    }

    /// ⭐ Field Day's runner, where the caller runs WSJT-X's Field Day activity: the caller's
    /// RR73 or RRR is logged and answered with 73 (Tx5), which goes once; then the station is
    /// done. A bare 73 from the caller, and a runner whose caller does not run WSJT-X's Field
    /// Day, close as they always have: logged, nothing sent back. A confirmation from anyone else
    /// closes nothing, as WSJT-X acts only on its QSO partner's (`processMessage`,
    /// mainwindow.cpp 9107-9110): nothing is logged, nothing goes to that station, the caller
    /// still gets the rogered exchange, and the caller's own RR73 then closes the contact.
    #[test]
    fn field_days_runner_answers_its_callers_rr73_with_73_in_wsjtx_field_day() {
        let runner = |wsjtx: bool| {
            let mut run = FieldDayStation::running(
                "W9XYZ",
                "EN37",
                ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
                "20M",
            );
            run.set_wsjtx_field_day(wsjtx);
            run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0);
            assert_eq!(next(&run), "K2DEF W9XYZ R 3A WI");
            run
        };
        for closing in ["W9XYZ K2DEF RR73", "W9XYZ K2DEF RRR"] {
            let mut run = runner(true);
            run.observe(&[dec(closing)], 1);
            assert_eq!(run.log.qso_count(), 1, "{closing}: logged");
            assert_eq!(next(&run), "K2DEF W9XYZ 73", "{closing}: answered with 73");
            assert!(!run.done(), "{closing}: the 73 is still to go");
            run.after_tx();
            assert!(run.done(), "{closing}: it went once");
            assert_eq!(next(&run), "", "{closing}");
        }
        for (wsjtx, closing) in [
            (true, "W9XYZ K2DEF 73"),
            (false, "W9XYZ K2DEF RR73"),
            (false, "W9XYZ K2DEF RRR"),
        ] {
            let mut run = runner(wsjtx);
            run.observe(&[dec(closing)], 1);
            assert_eq!(run.log.qso_count(), 1, "{wsjtx} {closing}: logged");
            assert!(run.done(), "{wsjtx} {closing}: done, nothing sent back");
        }
        // Each row as its call, class and section.
        let rows = |run: &FieldDayStation| -> Vec<String> {
            let row = |q: &LoggedQso| format!("{} {} {}", q.call, q.class(), q.section());
            run.log.qsos().iter().map(row).collect()
        };
        for wsjtx in [true, false] {
            for closing in ["W9XYZ N0ABC RR73", "W9XYZ N0ABC RRR", "W9XYZ N0ABC 73"] {
                let what = format!("{wsjtx} {closing}");
                let mut run = runner(wsjtx);
                run.observe(&[dec(closing)], 1);
                assert_eq!(rows(&run), Vec::<String>::new(), "{what}: nothing logged");
                assert_eq!(run.state, FdState::AwaitConfirm, "{what}: still K2DEF's");
                assert_eq!(
                    next(&run),
                    "K2DEF W9XYZ R 3A WI",
                    "{what}: nothing to N0ABC"
                );
                run.observe(&[dec("W9XYZ K2DEF RR73")], 3);
                assert_eq!(
                    rows(&run),
                    ["K2DEF 2A IL"],
                    "{what}: K2DEF's own RR73 logs it"
                );
                let close = if wsjtx { "K2DEF W9XYZ 73" } else { "" };
                assert_eq!(next(&run), close, "{what}: and closes as before");
            }
        }
    }

    /// ⭐ Field Day's S&P completes a contact only with the station it called, as WSJT-X acts only
    /// on its QSO partner's message: `auto_sequence` passes a message to us on to
    /// `processMessage` only when its sender holds the DX call (mainwindow.cpp 7546-7549), and
    /// `processMessage` acts only on its partner's (9107-9110). With W9XYZ called by the
    /// operator's double-click on its CQ, a rogered exchange to us from N0ABC, or
    /// from a call the decoder could not resolve (`<...>`), logs nothing and sends that station
    /// nothing: my exchange goes to W9XYZ again. W9XYZ's own then logs W9XYZ and is closed with
    /// RR73, as before. So in ARRL Field Day on FT8 and FT4, where the station runs WSJT-X's Field
    /// Day activity, on FT1, and in Winter Field Day. Heard in the same period, N0ABC's costs W9XYZ
    /// nothing; a nonstandard call of mine, which the exchanges name hashed, changes none of it;
    /// and W9XYZ's call hashed is still W9XYZ's, by its base call.
    #[test]
    fn field_days_sp_completes_a_contact_only_with_the_station_it_called() {
        // Each row as its call, class and section.
        let rows = |st: &FieldDayStation| -> Vec<String> {
            let row = |q: &LoggedQso| format!("{} {} {}", q.call, q.class(), q.section());
            st.log.qsos().iter().map(row).collect()
        };
        // W9XYZ called by `me` with a double-click on its CQ, and my exchange to it gone out.
        let called = |me: &str, event: FdEvent, class: &str, wsjtx: bool| {
            let session = ContestSession::field_day(event, class, "EMA");
            let mut sp = FieldDayStation::search_and_pounce(me, "FN42", session, "20M");
            sp.set_wsjtx_field_day(wsjtx);
            sp.answer_cq("W9XYZ", "EN37");
            sp.after_tx();
            sp
        };
        // (where, its event, my class, W9XYZ's and N0ABC's, the station runs WSJT-X's Field Day)
        for (place, event, mine, theirs, wsjtx) in [
            ("ARRL Field Day FT8/FT4", FdEvent::ArrlFd, "2B", "1D", true),
            ("ARRL Field Day FT1", FdEvent::ArrlFd, "2B", "1D", false),
            ("Winter Field Day", FdEvent::WinterFd, "1H", "1H", false),
        ] {
            let exch = format!("W9XYZ K1ABC {mine} EMA");
            let own = format!("K1ABC W9XYZ R {theirs} WI");
            let row = format!("W9XYZ {theirs} WI");
            let mut sp = called("K1ABC", event, mine, wsjtx);
            for other in ["N0ABC", "<...>"] {
                let what = format!("{place}, {other}'s");
                sp.observe(&[dec(&format!("K1ABC {other} R {theirs} WI"))], 1);
                assert_eq!(rows(&sp), Vec::<String>::new(), "{what}: nothing logged");
                assert_eq!(sp.state, FdState::AwaitExchange, "{what}: still W9XYZ's");
                assert_eq!(next(&sp), exch, "{what}: nothing to {other}");
                sp.after_tx();
            }
            sp.observe(&[dec(&own)], 3);
            assert_eq!(rows(&sp), [row.as_str()], "{place}: W9XYZ's own logs W9XYZ");
            assert_eq!(next(&sp), "W9XYZ K1ABC RR73", "{place}: and gets RR73");
            sp.after_tx();
            assert!(sp.done(), "{place}: once");
            // N0ABC's and W9XYZ's in the same period, either order.
            let stranger = dec(&format!("K1ABC N0ABC R {theirs} WI"));
            for (order, both) in [
                ("N0ABC's first", [stranger.clone(), dec(&own)]),
                ("W9XYZ's first", [dec(&own), stranger]),
            ] {
                let mut sp = called("K1ABC", event, mine, wsjtx);
                sp.observe(&both, 1);
                assert_eq!(rows(&sp), [row.as_str()], "{place}, {order}: W9XYZ logged");
                assert_eq!(next(&sp), "W9XYZ K1ABC RR73", "{place}, {order}: its RR73");
            }
        }
        // A nonstandard call of mine, which the rogered exchanges to me name hashed.
        let mut sp = called("PJ4/K1ABC", FdEvent::ArrlFd, "2B", true);
        sp.observe(&[dec("<PJ4/K1ABC> N0ABC R 1D WI")], 1);
        assert_eq!(
            rows(&sp),
            Vec::<String>::new(),
            "PJ4/K1ABC, N0ABC's: nothing logged"
        );
        assert_eq!(
            next(&sp),
            "W9XYZ PJ4/K1ABC 2B EMA",
            "PJ4/K1ABC, N0ABC's: W9XYZ's again"
        );
        sp.after_tx();
        sp.observe(&[dec("<PJ4/K1ABC> W9XYZ R 1D WI")], 3);
        assert_eq!(
            rows(&sp),
            ["W9XYZ 1D WI"],
            "PJ4/K1ABC: W9XYZ's own logs W9XYZ"
        );
        assert_eq!(
            next(&sp),
            "<W9XYZ> PJ4/K1ABC RR73",
            "PJ4/K1ABC: and gets RR73"
        );
        // W9XYZ's call hashed is W9XYZ's, by its base call: the contact closes as it always has,
        // the RR73 naming the call as the decoder printed it and the row without the brackets.
        let mut sp = called("K1ABC", FdEvent::ArrlFd, "2B", true);
        sp.observe(&[dec("K1ABC <W9XYZ> R 1D WI")], 1);
        assert_eq!(rows(&sp), ["W9XYZ 1D WI"], "<W9XYZ>'s: logged bare");
        assert_eq!(next(&sp), "<W9XYZ> K1ABC RR73", "<W9XYZ>'s: RR73 as before");
    }

    /// The control: a station starts on Field Day's messages, and naming them again moves
    /// nothing — not the CQ, not a contact in flight.
    #[test]
    fn field_day_messages_are_where_every_station_starts_and_stay_put() {
        let mut run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        assert_eq!(run.messages(), FtMessages::FieldDay);
        assert!(!run.sends_grid(), "Field Day's exchange is grid-exempt");
        run.set_messages(FtMessages::FieldDay, true);
        assert_eq!(next(&run), "CQ FD W9XYZ EN37");
        run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0);
        run.set_messages(FtMessages::FieldDay, true);
        assert_eq!(
            run.state,
            FdState::AwaitConfirm,
            "the contact is still in flight"
        );
        assert_eq!(next(&run), "K2DEF W9XYZ R 3A WI");
    }

    /// ⭐ The ARRL VHF contests, Running: `CQ TEST`, `R <grid>` to the caller's grid, and 73 to
    /// its RR73 — WSJT-X's NA VHF Contest sequence. The caller's grid is logged as the GRID it
    /// sent, and mine as the GRID I sent.
    #[test]
    fn vhf_running_calls_cq_test_rogers_the_grid_and_closes_with_73() {
        let mut run = ft8_station("arrlvhf_jan", "FN42", true);
        assert!(run.sends_grid());
        assert_eq!(next(&run), "CQ TEST K1ABC FN42");
        run.observe(&[dec("K1ABC W9XYZ EN37")], 1);
        assert_eq!(run.state, FdState::AwaitConfirm);
        assert_eq!(next(&run), "W9XYZ K1ABC R FN42");
        assert_eq!(
            run.log.qsos().len(),
            0,
            "nothing logged before the confirmation"
        );
        run.observe(&[dec("K1ABC W9XYZ RR73")], 3);
        assert_eq!(run.state, FdState::Done);
        assert_eq!(next(&run), "W9XYZ K1ABC 73");
        assert_eq!(
            last_row(&run),
            (
                "W9XYZ".into(),
                vec!["GRID=EN37".into()],
                vec!["GRID=FN42".into()]
            )
        );
        assert_eq!(run.log.qsos()[0].mode, "DIG");
        // The 73 goes out once; then the engine re-arms the run onto its CQ.
        run.after_tx();
        assert!(run.done());
        run.rearm(true);
        assert_eq!(next(&run), "CQ TEST K1ABC FN42");
        // RRR closes it the same way (WSJT-X's Tx4 is RR73 or RRR).
        run.observe(&[dec("K1ABC N0ABC EM48")], 5);
        run.observe(&[dec("K1ABC N0ABC RRR")], 7);
        assert_eq!(next(&run), "N0ABC K1ABC 73");
        assert_eq!(run.log.qsos().len(), 2);
    }

    /// ⭐ The ARRL VHF contests, S&P: a CQ is answered with my grid, and the runner's
    /// `R <grid>` is logged and closed with RR73.
    #[test]
    fn vhf_sp_answers_with_its_grid_and_closes_with_rr73() {
        let mut sp = ft8_station("arrlvhf_jun", "FN42", false);
        assert_eq!(
            next(&sp),
            "",
            "S&P sends nothing until a CQ is double-clicked"
        );
        click(&mut sp, "CQ TEST W9XYZ EN37");
        assert_eq!(sp.state, FdState::AwaitExchange);
        assert_eq!(next(&sp), "W9XYZ K1ABC FN42");
        sp.observe(&[dec("K1ABC W9XYZ R EN37")], 3);
        assert_eq!(sp.state, FdState::Done);
        assert_eq!(next(&sp), "W9XYZ K1ABC RR73");
        assert_eq!(
            last_row(&sp),
            (
                "W9XYZ".into(),
                vec!["GRID=EN37".into()],
                vec!["GRID=FN42".into()]
            )
        );
        // Back to listening, no CQ heard is answered: the same runner's, nor the same runner's
        // in a new grid.
        sp.after_tx();
        sp.rearm(false);
        sp.observe(&[dec("CQ TEST W9XYZ EN37")], 5);
        assert_eq!((sp.state, next(&sp)), (FdState::Listening, String::new()));
        sp.observe(&[dec("CQ TEST W9XYZ EN38")], 7);
        assert_eq!((sp.state, next(&sp)), (FdState::Listening, String::new()));
        // A plain CQ double-clicked is answered the same way, as Field Day's is.
        let mut sp = ft8_station("arrlvhf_sep", "FN42", false);
        click(&mut sp, "CQ W9XYZ EN37");
        assert_eq!(next(&sp), "W9XYZ K1ABC FN42");
    }

    /// What WSJT-X would not answer, neither does this: a CQ with no grid, a grid sent to
    /// somebody else, and a confirmation from anybody but the station being worked.
    #[test]
    fn vhf_answers_only_its_own_contact() {
        let mut sp = ft8_station("arrlvhf_jan", "FN42", false);
        sp.observe(&[dec("CQ TEST PJ4/K1XYZ")], 1);
        assert_eq!((sp.state, next(&sp)), (FdState::Listening, String::new()));
        click(&mut sp, "CQ TEST W9XYZ EN37");
        sp.observe(&[dec("K1ABC N0ABC R EM48")], 5);
        sp.observe(&[dec("N0ABC W9XYZ R EN37")], 5);
        sp.observe(&[dec("K1ABC W9XYZ R RR73")], 5);
        assert_eq!(
            sp.state,
            FdState::AwaitExchange,
            "none of those is my roger"
        );
        assert!(sp.log.qsos().is_empty());

        let mut run = ft8_station("arrlvhf_jan", "FN42", true);
        run.observe(&[dec("N0ABC W9XYZ EN37")], 1);
        assert_eq!(
            run.state,
            FdState::CallingCq,
            "a grid sent to somebody else"
        );
        run.observe(&[dec("<K1ABC> PJ4/W9XYZ")], 1);
        assert_eq!(run.state, FdState::CallingCq, "a call with no grid");
        run.observe(&[dec("K1ABC W9XYZ EN37")], 3);
        run.observe(&[dec("K1ABC N0ABC RR73")], 5);
        assert_eq!(run.state, FdState::AwaitConfirm, "an RR73 from a bystander");
        assert!(run.log.qsos().is_empty());
        run.observe(&[dec("K1ABC W9XYZ RR73")], 7);
        assert_eq!(
            run.log.qsos().len(),
            1,
            "the partner's RR73 still closes it"
        );
    }

    /// ⛔ The ARRL VHF and standard messages' S&P passes by a CQ from a call the decoder could not
    /// resolve, as WSJT-X builds no message to one: it takes a CQ's call from the word after it
    /// (`deCallAndGrid`, decodedtext.cpp 212-224), which `...` cannot be, and puts only a
    /// callsign into its DX call box (`Radio::is_callsign`, mainwindow.cpp 9402). The station's
    /// next CQ, its call resolved, is answered as before once double-clicked.
    #[test]
    fn vhf_and_standard_sp_pass_by_a_cq_from_an_unresolved_call() {
        for (event, cq) in [
            ("arrlvhf_jan", "CQ TEST"),
            ("txqp", "CQ TEST"),
            ("txqp", "CQ QP"),
        ] {
            let mut sp = ft8_station(event, "FN42", false);
            sp.observe(&[dec(&format!("{cq} <...> EN37"))], 1);
            assert_eq!(
                (sp.state, next(&sp)),
                (FdState::Listening, String::new()),
                "{event} {cq}: passed by"
            );
            click(&mut sp, &format!("{cq} <K9QQQ/M> EN37"));
            assert_eq!(
                (sp.state, next(&sp)),
                (FdState::AwaitExchange, "<K9QQQ/M> K1ABC FN42".to_string()),
                "{event} {cq}: its next CQ, resolved, is answered"
            );
        }
    }

    /// ⛔ The ARRL VHF and standard messages' Running passes by a caller whose call the decoder
    /// could not resolve, its grid or its report, as WSJT-X builds no message to one: its messages
    /// go to the call in its DX call box (`genStdMsgs`, mainwindow.cpp 9567-9577), which takes
    /// only a callsign (`Radio::is_callsign`, 9402). The next caller in the same period, its call
    /// resolved, is answered.
    #[test]
    fn vhf_and_standard_runs_pass_by_an_unresolved_caller() {
        for (event, caller, resolved, state, answer) in [
            (
                "arrlvhf_jan",
                "K1ABC <...> EN37",
                "K1ABC <K9QQQ/M> EN37",
                FdState::AwaitConfirm,
                "<K9QQQ/M> K1ABC R FN42",
            ),
            (
                "txqp",
                "K1ABC <...> EN37",
                "K1ABC <K9QQQ/M> EN37",
                FdState::AwaitExchange,
                "<K9QQQ/M> K1ABC -05",
            ),
            (
                "txqp",
                "K1ABC <...> -12",
                "K1ABC <K9QQQ/M> -12",
                FdState::AwaitConfirm,
                "<K9QQQ/M> K1ABC R-05",
            ),
        ] {
            let mut run = ft8_station(event, "FN42", true);
            let cq = next(&run);
            run.observe(&[dec(caller)], 1);
            assert_eq!(
                (run.state, next(&run)),
                (FdState::CallingCq, cq),
                "{event} {caller}: passed by"
            );
            run.observe(&[dec(caller), dec(resolved)], 3);
            assert_eq!(
                (run.state, next(&run)),
                (state, answer.to_string()),
                "{event} {caller}: the next caller, resolved, is answered"
            );
        }
    }

    #[test]
    fn rogered_grid_is_read_only_in_the_packers_own_form() {
        let ok = |t: &str| rogered_grid_of(t).map(|(a, b, g)| format!("{a} {b} {g}"));
        assert_eq!(
            ok("K1ABC W9XYZ R EN37").as_deref(),
            Some("K1ABC W9XYZ EN37")
        );
        // A nonstandard call it goes to comes hashed, as the decoder prints it.
        assert_eq!(
            ok("<PJ4/K1ABC> W9XYZ R EN37").as_deref(),
            Some("<PJ4/K1ABC> W9XYZ EN37")
        );
        assert_eq!(
            rogered_grid("W9XYZ", "K1ABC", "FN42").to_text(),
            "W9XYZ K1ABC R FN42"
        );
        for not in [
            "K1ABC W9XYZ R RR73",
            "K1ABC W9XYZ R EN37AB",
            "K1ABC W9XYZ R SA00",
            "K1ABC W9XYZ R AS00",
            "K1ABC W9XYZ EN37",
            "K1ABC W9XYZ R 3A WI",
            "K1ABC DE R EN37",
            "K1ABC W9XYZ R-07",
        ] {
            assert_eq!(ok(not), None, "{not}");
        }
    }

    /// ⭐ A contest WSJT-X has no activity for, both roles: the standard messages, the
    /// report being the SNR the other station was decoded at. The row carries the call and
    /// nothing of the contest's exchange, because none of it went either way.
    #[test]
    fn a_contest_without_an_activity_runs_the_standard_messages() {
        let mut run = ft8_station("txqp", "FN42", true);
        assert!(run.sends_grid());
        assert_eq!(next(&run), "CQ K1ABC FN42");
        run.observe(&[dec("K1ABC W9XYZ EN37")], 1);
        assert_eq!(next(&run), "W9XYZ K1ABC -05");
        run.observe(&[dec("K1ABC W9XYZ R-07")], 3);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::Done, "W9XYZ K1ABC RR73".into())
        );
        assert_eq!(last_row(&run), ("W9XYZ".into(), vec![], vec![]));

        let mut sp = ft8_station("ohqp", "FN42", false);
        click(&mut sp, "CQ W9XYZ EN37");
        assert_eq!(next(&sp), "W9XYZ K1ABC FN42");
        sp.observe(&[dec("K1ABC W9XYZ -07")], 3);
        assert_eq!(next(&sp), "W9XYZ K1ABC R-05");
        assert!(sp.log.qsos().is_empty(), "nothing logged before the RR73");
        sp.observe(&[dec("K1ABC W9XYZ RR73")], 5);
        assert_eq!(
            (sp.state, next(&sp)),
            (FdState::Done, "W9XYZ K1ABC 73".into())
        );
        assert_eq!(last_row(&sp), ("W9XYZ".into(), vec![], vec![]));
        // Back to listening, S&P does not answer the same station's next CQ.
        sp.after_tx();
        sp.rearm(false);
        sp.observe(&[dec("CQ W9XYZ EN37")], 7);
        assert_eq!((sp.state, next(&sp)), (FdState::Listening, String::new()));
    }

    /// A caller that sends its report without its grid is rogered (Tx3), as WSJT-X does and
    /// as the standard sequencer does outside a contest.
    #[test]
    fn a_standard_runner_rogers_a_caller_that_skipped_its_grid() {
        let mut run = ft8_station("cqp", "FN42", true);
        run.observe(&[dec("K1ABC W9XYZ -07")], 1);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::AwaitConfirm, "W9XYZ K1ABC R-05".into())
        );
        run.observe(&[dec("K1ABC W9XYZ RR73")], 3);
        assert_eq!(next(&run), "W9XYZ K1ABC 73");
        assert_eq!(run.log.qsos().len(), 1);
    }

    /// The CQ a running station calls, by value, for each set of messages: Field Day's with the
    /// locator as typed and never a contest GRID, the others with the contest's own GRID or the
    /// locator's four characters; and, as the Tx6 row shows it, with four characters on the air.
    #[test]
    fn the_running_cq_by_value() {
        let cq = |m: FtMessages, session: &str| m.cq("K1ABC", "FN42ab", session, "").to_text();
        assert_eq!(cq(FtMessages::FieldDay, ""), "CQ FD K1ABC FN42ab");
        assert_eq!(cq(FtMessages::FieldDay, "EN52"), "CQ FD K1ABC FN42ab");
        assert_eq!(cq(FtMessages::NaVhf, ""), "CQ TEST K1ABC FN42");
        assert_eq!(cq(FtMessages::NaVhf, "EN52"), "CQ TEST K1ABC EN52");
        assert_eq!(cq(FtMessages::Standard, ""), "CQ K1ABC FN42");
        assert_eq!(cq(FtMessages::Standard, "EN52"), "CQ K1ABC EN52");
        assert_eq!(
            FtMessages::FieldDay.cq_on_air("K1ABC", "FN42ab", ""),
            "CQ FD K1ABC FN42"
        );
        // A direction typed in Tx6 is the CQ only where WSJT-X has no activity, and never a
        // nonstandard call's.
        let typed = |m: FtMessages, call: &str| m.cq(call, "FN42ab", "", "QP").to_text();
        assert_eq!(typed(FtMessages::Standard, "K1ABC"), "CQ QP K1ABC FN42");
        assert_eq!(typed(FtMessages::FieldDay, "K1ABC"), "CQ FD K1ABC FN42ab");
        assert_eq!(typed(FtMessages::NaVhf, "K1ABC"), "CQ TEST K1ABC FN42");
        let nonstandard = typed(FtMessages::Standard, "PJ4/K1ABC");
        assert!(!nonstandard.contains("QP"), "{nonstandard}");
        // A nonstandard call calls WSJT-X's plain CQ whatever the messages (`genCQMsg`: no grid,
        // no token, for a call `stdCall` refuses), and the running station starts with it.
        for m in [
            FtMessages::FieldDay,
            FtMessages::NaVhf,
            FtMessages::Standard,
        ] {
            for call in ["PJ4/K1ABC", "K1ABC/QRP"] {
                let cq = m.cq(call, "FN42ab", "EN52", "QP").to_text();
                assert_eq!(cq, format!("CQ {call}"), "{m:?}");
            }
        }
        let data = crate::contest::StationData {
            mycall: "PJ4/K1ABC".into(),
            mygrid: "FN42".into(),
            contest_qth_state: "MA".into(),
            ..Default::default()
        };
        let rs = crate::fd_rules::ruleset_by_id("txqp", crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        let run = FieldDayStation::running("PJ4/K1ABC", "FN42", session, "20M");
        assert_eq!(
            run.pending.as_ref().map(Msg::to_text).as_deref(),
            Some("CQ PJ4/K1ABC"),
            "the first CQ of a new Running"
        );
    }

    /// The station `mycall` runs in `event`: Field Day's with its own exchange, `2B EMA`, and a
    /// caller running WSJT-X's Field Day activity; the other contests' from the rules file.
    fn contest_station(mycall: &str, event: &str, running: bool) -> FieldDayStation {
        if event != "arrlfd" {
            return ft8_station_as(mycall, event, "FN42", running);
        }
        let session = ContestSession::field_day(FdEvent::ArrlFd, "2B", "EMA");
        let mut st = if running {
            FieldDayStation::running(mycall, "FN42", session, "20M")
        } else {
            FieldDayStation::search_and_pounce(mycall, "FN42", session, "20M")
        };
        st.set_wsjtx_field_day(true);
        st
    }

    /// One contact by `mycall` in `event`: each step is what is heard (nothing, for a Running's
    /// first CQ) and what goes on the air next. It must end logged, with W9XYZ's call bare.
    fn contact(mycall: &str, event: &str, running: bool, steps: &[(String, String)]) {
        let mut st = contest_station(mycall, event, running);
        for (slot, (heard, sent)) in steps.iter().enumerate() {
            if !running && st.state == FdState::Listening {
                click(&mut st, heard);
            } else if !heard.is_empty() {
                st.observe(&[dec(heard)], slot as u64);
            }
            assert_eq!(
                &next(&st),
                sent,
                "{mycall} {event} running={running}, {heard:?}"
            );
        }
        assert_eq!(
            st.state,
            FdState::Done,
            "{mycall} {event} running={running}"
        );
        assert_eq!(
            last_row(&st).0,
            "W9XYZ",
            "{mycall} {event} running={running}"
        );
    }

    /// ⭐ A nonstandard call (a prefix, or any suffix but `/P` and `/R`) goes on the air in every
    /// over of a contest contact as WSJT-X 3.0.2 writes it (`genStdMsgs`, mainwindow.cpp
    /// 9596-9597, 9601, 9625-9627, 9655-9657, 9702, 9710): hashed beside a grid or a report;
    /// whole beside the other station's hashed call in RR73, 73 and the standard messages'
    /// answer to a CQ, which then carries no grid; and whole in Field Day's exchange, which its
    /// packer hashes. What is heard is what the decoder prints from a station answering that
    /// call: mine hashed beside its grid, report or exchange, its own hashed in its RR73 and 73.
    #[test]
    fn a_nonstandard_call_goes_out_as_wsjtx_writes_it() {
        for me in ["PJ4/K1ABC", "K1ABC/QRP"] {
            let h = format!("<{me}>");
            let s = |heard: &str, sent: String| (heard.to_string(), sent);
            let fd_sp = [
                s("CQ FD W9XYZ EN37", format!("W9XYZ {me} 2B EMA")),
                s(&format!("{h} W9XYZ R 1D WI"), format!("<W9XYZ> {me} RR73")),
            ];
            let fd_run = [
                s("", format!("CQ {me}")),
                s(&format!("{h} W9XYZ 1D WI"), format!("W9XYZ {me} R 2B EMA")),
                s(&format!("{me} <W9XYZ> RR73"), format!("<W9XYZ> {me} 73")),
            ];
            let vhf_sp = [
                s("CQ TEST W9XYZ EN37", format!("W9XYZ {h} FN42")),
                s(&format!("{h} W9XYZ R EN37"), format!("<W9XYZ> {me} RR73")),
            ];
            let vhf_run = [
                s("", format!("CQ {me}")),
                s(&format!("{h} W9XYZ EN37"), format!("W9XYZ {h} R FN42")),
                s(&format!("{me} <W9XYZ> RR73"), format!("<W9XYZ> {me} 73")),
            ];
            let std_sp = [
                s("CQ W9XYZ EN37", format!("<W9XYZ> {me}")),
                s(&format!("{h} W9XYZ -07"), format!("W9XYZ {h} R-05")),
                s(&format!("{me} <W9XYZ> RR73"), format!("<W9XYZ> {me} 73")),
            ];
            let std_run = [
                s("", format!("CQ {me}")),
                s(&format!("{h} W9XYZ EN37"), format!("W9XYZ {h} -05")),
                s(&format!("{h} W9XYZ R-07"), format!("<W9XYZ> {me} RR73")),
            ];
            contact(me, "arrlfd", false, &fd_sp);
            contact(me, "arrlfd", true, &fd_run);
            contact(me, "arrlvhf_jun", false, &vhf_sp);
            contact(me, "arrlvhf_jun", true, &vhf_run);
            contact(me, "txqp", false, &std_sp);
            contact(me, "txqp", true, &std_run);
        }
    }

    /// The control: a standard call, `/P` included (`stdCall` passes it), goes on the air exactly
    /// as before, and Field Day still takes only a reply naming it whole. Its hashed form reaches
    /// a standard call only from a station whose own call is nonstandard, which this leaves as it
    /// was.
    #[test]
    fn a_standard_call_goes_out_and_is_heard_exactly_as_before() {
        for me in ["K1ABC", "K1ABC/P"] {
            let s = |heard: String, sent: String| (heard, sent);
            let fd_sp = [
                s("CQ FD W9XYZ EN37".into(), format!("W9XYZ {me} 2B EMA")),
                s(format!("{me} W9XYZ R 1D WI"), format!("W9XYZ {me} RR73")),
            ];
            let fd_run = [
                s(String::new(), format!("CQ FD {me} FN42")),
                s(format!("{me} W9XYZ 1D WI"), format!("W9XYZ {me} R 2B EMA")),
                s(format!("{me} W9XYZ RR73"), format!("W9XYZ {me} 73")),
            ];
            let vhf_sp = [
                s("CQ TEST W9XYZ EN37".into(), format!("W9XYZ {me} FN42")),
                s(format!("{me} W9XYZ R EN37"), format!("W9XYZ {me} RR73")),
            ];
            let vhf_run = [
                s(String::new(), format!("CQ TEST {me} FN42")),
                s(format!("{me} W9XYZ EN37"), format!("W9XYZ {me} R FN42")),
                s(format!("{me} W9XYZ RR73"), format!("W9XYZ {me} 73")),
            ];
            let std_sp = [
                s("CQ W9XYZ EN37".into(), format!("W9XYZ {me} FN42")),
                s(format!("{me} W9XYZ -07"), format!("W9XYZ {me} R-05")),
                s(format!("{me} W9XYZ RR73"), format!("W9XYZ {me} 73")),
            ];
            let std_run = [
                s(String::new(), format!("CQ {me} FN42")),
                s(format!("{me} W9XYZ EN37"), format!("W9XYZ {me} -05")),
                s(format!("{me} W9XYZ R-07"), format!("W9XYZ {me} RR73")),
            ];
            contact(me, "arrlfd", false, &fd_sp);
            contact(me, "arrlfd", true, &fd_run);
            contact(me, "arrlvhf_jun", false, &vhf_sp);
            contact(me, "arrlvhf_jun", true, &vhf_run);
            contact(me, "txqp", false, &std_sp);
            contact(me, "txqp", true, &std_run);
        }
        let mut run = contest_station("K1ABC", "arrlfd", true);
        run.observe(&[dec("<K1ABC> W9XYZ 1D WI")], 1);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::CallingCq, "CQ FD K1ABC FN42".to_string()),
            "a standard call's hashed form is not taken"
        );
    }

    /// Every CALL an ADIF export writes, in order.
    fn adif_calls(adif: &str) -> Vec<String> {
        adif.split("<CALL:")
            .skip(1)
            .filter_map(|r| {
                let (len, rest) = r.split_once('>')?;
                rest.get(..len.parse().ok()?).map(str::to_string)
            })
            .collect()
    }

    /// ⭐ Field Day's messages log a call that came hashed without its brackets, as the ARRL VHF
    /// and standard messages do ([`FieldDayStation::log_worked`]) and as WSJT-X logs the call in
    /// its DX call box, which a decoded message fills with the brackets dropped (`DecodedText`,
    /// decodedtext.cpp 66). A nonstandard call (any suffix but `/P` and `/R`) packs its Field Day
    /// exchange with the call hashed, so a runner on one rogers S&P's exchange as
    /// `K1ABC <W9XYZ/M> R 1D WI`, and a caller on one sends the run `K1ABC <W9XYZ/M> 1D WI`. In
    /// S&P, in the run closed by RR73 or 73, and where the caller rogers first, the rows are the
    /// rows the same contact logs with the call heard whole (the call, the entity and prefix
    /// resolved from it, the exchange, the dupe mark), the submitted ADIF and the journal write
    /// the call, and S&P, listening again, answers none of the station's next CQs. What
    /// goes on the air is what it was, but RR73 and 73 to a nonstandard call, which go as WSJT-X
    /// writes them (`rr73_and_73_to_a_nonstandard_call_go_out_as_wsjtx_writes_them`). The call
    /// heard whole is the control.
    #[test]
    fn field_days_messages_log_a_hashed_call_without_its_brackets() {
        // The rows, the time set aside, and the calls the submitted ADIF and the journal write.
        let logged = |st: &FieldDayStation| {
            let mut rows = st.log.qsos().to_vec();
            for q in &mut rows {
                q.when_unix = 0;
            }
            let calls = |adif: String| adif_calls(&adif);
            (
                rows,
                calls(st.log.submission_adif()),
                calls(st.log.journal_adif()),
            )
        };
        // The same for `call` logged whole off the same exchange, in the same slot.
        let whole = |call: &str, running: bool, slot: u64| {
            let mut st = contest_station("K1ABC", "arrlfd", running);
            st.log.log(call, "1D", "WI", slot);
            logged(&st)
        };
        // (the call, as the exchanges name it)
        for (call, heard) in [
            ("W9XYZ", "W9XYZ"),
            ("W9XYZ", "<W9XYZ>"),
            ("W9XYZ/M", "<W9XYZ/M>"),
            ("W9XYZ/4", "<W9XYZ/4>"),
            ("W9XYZ/QRP", "<W9XYZ/QRP>"),
        ] {
            // S&P, on the CQ the call sends: a nonstandard call's carries no grid.
            let cq = if crate::message::is_std_call(call) {
                format!("CQ FD {call} EN37")
            } else {
                format!("CQ {call}")
            };
            // The two calls of RR73 and 73.
            let closing = if crate::message::is_std_call(call) {
                format!("{heard} K1ABC")
            } else {
                format!("{call} <K1ABC>")
            };
            let what = format!("{heard} S&P");
            let mut sp = contest_station("K1ABC", "arrlfd", false);
            click(&mut sp, &cq);
            assert_eq!(
                next(&sp),
                format!("{call} K1ABC 2B EMA"),
                "{what}: the answer"
            );
            sp.after_tx();
            sp.observe(&[dec(&format!("K1ABC {heard} R 1D WI"))], 2);
            assert_eq!(next(&sp), format!("{closing} RR73"), "{what}: RR73");
            assert_eq!(logged(&sp), whole(call, false, 2), "{what}: logged");
            sp.after_tx();
            sp.rearm(false);
            sp.observe(&[dec(&cq)], 4);
            assert_eq!(
                (sp.state, next(&sp)),
                (FdState::Listening, String::new()),
                "{what}: its next CQ heard is not answered"
            );
            // Running: the caller's exchange, then its RR73 or 73.
            for close in ["RR73", "73"] {
                let what = format!("{heard} Running, {close}");
                let mut run = contest_station("K1ABC", "arrlfd", true);
                run.observe(&[dec(&format!("K1ABC {heard} 1D WI"))], 1);
                assert_eq!(
                    next(&run),
                    format!("{heard} K1ABC R 2B EMA"),
                    "{what}: R and mine"
                );
                run.after_tx();
                run.observe(&[dec(&format!("K1ABC {heard} {close}"))], 3);
                let bye = match close {
                    "RR73" => format!("{closing} 73"),
                    _ => String::new(),
                };
                assert_eq!(next(&run), bye, "{what}: the close");
                assert_eq!(logged(&run), whole(call, true, 3), "{what}: logged");
            }
            // Running: a caller that rogers its exchange first.
            let what = format!("{heard} Running, rogered first");
            let mut run = contest_station("K1ABC", "arrlfd", true);
            run.observe(&[dec(&format!("K1ABC {heard} R 1D WI"))], 1);
            assert_eq!(next(&run), format!("{closing} RR73"), "{what}: RR73");
            assert_eq!(logged(&run), whole(call, true, 1), "{what}: logged");
        }
    }

    /// ⭐ RR73 AND 73 TO A NONSTANDARD CALL GO OUT AS WSJT-X 3.0.2 WRITES THEM in Field Day's
    /// messages: that call whole and mine hashed, `W9XYZ/M <K1ABC> RR73` (`genStdMsgs`,
    /// mainwindow.cpp 9701 and 9709, `if(!bHisCall and bMyCall) t=hisCall + " <" + my_callsign +
    /// "> " …`), where `hisCall` is the call the decoder printed, brackets dropped (`DecodedText`,
    /// decodedtext.cpp 66). So S&P's RR73 to a runner signing W9XYZ/M, the run's 73 to such a
    /// caller's RR73, and the run's RR73 to one that rogers first, with a `/P` call of mine too
    /// (`stdCall` passes it, 9588). The exchange overs before them, S&P's answer and the run's R
    /// and exchange, are what they were.
    #[test]
    fn rr73_and_73_to_a_nonstandard_call_go_out_as_wsjtx_writes_them() {
        for me in ["K1ABC", "K1ABC/P"] {
            for his in ["W9XYZ/M", "W9XYZ/4", "W9XYZ/QRP", "PJ4/W9XYZ"] {
                let h = format!("<{his}>");
                let what = format!("{me} working {his}");
                let mut sp = contest_station(me, "arrlfd", false);
                click(&mut sp, &format!("CQ {his}"));
                assert_eq!(
                    next(&sp),
                    format!("{his} {me} 2B EMA"),
                    "{what}: S&P's answer"
                );
                sp.after_tx();
                sp.observe(&[dec(&format!("{me} {h} R 1D WI"))], 2);
                assert_eq!(
                    next(&sp),
                    format!("{his} <{me}> RR73"),
                    "{what}: S&P's RR73"
                );
                assert_eq!(last_row(&sp).0, his, "{what}: S&P logged");

                let mut run = contest_station(me, "arrlfd", true);
                run.observe(&[dec(&format!("{me} {h} 1D WI"))], 1);
                assert_eq!(
                    next(&run),
                    format!("{h} {me} R 2B EMA"),
                    "{what}: the run's R and exchange"
                );
                run.after_tx();
                run.observe(&[dec(&format!("{me} {h} RR73"))], 3);
                assert_eq!(
                    next(&run),
                    format!("{his} <{me}> 73"),
                    "{what}: the run's 73"
                );
                assert_eq!(last_row(&run).0, his, "{what}: the run logged");

                let mut run = contest_station(me, "arrlfd", true);
                run.observe(&[dec(&format!("{me} {h} R 1D WI"))], 1);
                assert_eq!(
                    next(&run),
                    format!("{his} <{me}> RR73"),
                    "{what}: the run's RR73 to a caller that rogers first"
                );
            }
        }
    }

    /// ⭐ RR73 AND 73 TO A NONSTANDARD CALL GO OUT AS WSJT-X 3.0.2 WRITES THEM in the ARRL VHF
    /// and standard messages too: WSJT-X's Tx4 and Tx5 are the same lines in every activity
    /// (`genStdMsgs`, mainwindow.cpp 9701 and 9709), `W9XYZ/M <K1ABC> RR73` and `… 73`. A caller
    /// signing W9XYZ/M sends its grid or report with its call hashed, `K1ABC <W9XYZ/M> EN37`
    /// (`t0b`, 9597 and 9625-9626), and closes with mine hashed, `<K1ABC> W9XYZ/M RR73` (9702),
    /// or is heard closing with its own hashed. The run's 73 to it, and the standard messages'
    /// RR73 to one that rogers my report, take WSJT-X's form. Before, the 73 was
    /// `W9XYZ/M K1ABC 73`, which the packer sends as `W9XYZ K1ABC 73`: another station's call.
    #[test]
    fn rr73_and_73_to_a_nonstandard_call_go_out_as_wsjtx_writes_them_in_every_set() {
        for his in ["W9XYZ/M", "W9XYZ/4", "W9XYZ/QRP", "PJ4/W9XYZ"] {
            let h = format!("<{his}>");
            for close in [
                format!("<K1ABC> {his} RR73"),
                format!("<K1ABC> {his} RRR"),
                format!("K1ABC {h} RR73"),
            ] {
                let what = format!("ARRL VHF, {close}");
                let mut vhf = ft8_station("arrlvhf_jan", "FN42", true);
                vhf.observe(&[dec(&format!("K1ABC {h} EN37"))], 1);
                assert_eq!(
                    next(&vhf),
                    format!("{h} K1ABC R FN42"),
                    "{what}: R and mine"
                );
                vhf.after_tx();
                vhf.observe(&[dec(&close)], 3);
                assert_eq!(
                    (vhf.state, next(&vhf)),
                    (FdState::Done, format!("{his} <K1ABC> 73")),
                    "{what}: the 73"
                );

                let what = format!("standard messages, {close}");
                let mut std = ft8_station("txqp", "FN42", true);
                std.observe(&[dec(&format!("K1ABC {h} -07"))], 1);
                assert_eq!(next(&std), format!("{h} K1ABC R-05"), "{what}: R and mine");
                std.after_tx();
                std.observe(&[dec(&close)], 3);
                assert_eq!(
                    (std.state, next(&std)),
                    (FdState::Done, format!("{his} <K1ABC> 73")),
                    "{what}: the 73"
                );
            }
            let what = format!("standard messages, {his} rogers my report");
            let mut std = ft8_station("txqp", "FN42", true);
            std.observe(&[dec(&format!("K1ABC {h} EN37"))], 1);
            assert_eq!(next(&std), format!("{h} K1ABC -05"), "{what}: my report");
            std.after_tx();
            std.observe(&[dec(&format!("K1ABC {h} R-07"))], 3);
            assert_eq!(
                (std.state, next(&std)),
                (FdState::Done, format!("{his} <K1ABC> RR73")),
                "{what}: RR73"
            );
        }
    }

    /// The control: outside a standard call of mine closing to a nonstandard one, RR73 and 73
    /// go out as before. In Field Day's messages a standard call heard hashed, `<W9XYZ>`, and
    /// both calls nonstandard; in the ARRL VHF and standard messages a standard call, whole or
    /// heard hashed, and a `/P` call, which `stdCall` passes (mainwindow.cpp 9589). A nonstandard
    /// call of mine closing to a standard one is `a_nonstandard_call_goes_out_as_wsjtx_writes_it`'s.
    /// And only the caller's own close sends a 73: one to another call, from another station,
    /// or naming a call the decoder could not resolve closes nothing and sends no 73.
    #[test]
    fn rr73_and_73_otherwise_go_out_as_before() {
        let mut sp = contest_station("K1ABC", "arrlfd", false);
        click(&mut sp, "CQ FD W9XYZ EN37");
        sp.after_tx();
        sp.observe(&[dec("K1ABC <W9XYZ> R 1D WI")], 2);
        assert_eq!(
            next(&sp),
            "<W9XYZ> K1ABC RR73",
            "a standard call heard hashed"
        );
        let mut sp = contest_station("PJ4/K1ABC", "arrlfd", false);
        click(&mut sp, "CQ W9XYZ/M");
        sp.after_tx();
        sp.observe(&[dec("<PJ4/K1ABC> <W9XYZ/M> R 1D WI")], 2);
        assert_eq!(
            next(&sp),
            "<W9XYZ/M> PJ4/K1ABC RR73",
            "both calls nonstandard"
        );
        for event in ["arrlvhf_jan", "txqp"] {
            // (the caller as heard, my 73 to its RR73)
            for (heard, bye) in [
                ("W9XYZ", "W9XYZ K1ABC 73"),
                ("<W9XYZ>", "<W9XYZ> K1ABC 73"),
                ("W9XYZ/P", "W9XYZ/P K1ABC 73"),
            ] {
                let what = format!("{event}, {heard}");
                let mut run = ft8_station(event, "FN42", true);
                let call = match event {
                    "arrlvhf_jan" => format!("K1ABC {heard} EN37"),
                    _ => format!("K1ABC {heard} -07"),
                };
                run.observe(&[dec(&call)], 1);
                run.after_tx();
                run.observe(&[dec(&format!("K1ABC {heard} RR73"))], 3);
                assert_eq!(
                    (run.state, next(&run)),
                    (FdState::Done, bye.to_string()),
                    "{what}: the 73"
                );
            }
            for close in [
                "<K2DEF> W9XYZ/M RR73",
                "<K1ABC> N0ABC/M RR73",
                "K1ABC <...> RR73",
                "<...> W9XYZ/M RR73",
            ] {
                let what = format!("{event}, {close}");
                let mut run = ft8_station(event, "FN42", true);
                let call = match event {
                    "arrlvhf_jan" => "K1ABC <W9XYZ/M> EN37",
                    _ => "K1ABC <W9XYZ/M> -07",
                };
                run.observe(&[dec(call)], 1);
                let roger = next(&run);
                run.after_tx();
                run.observe(&[dec(close)], 3);
                assert_eq!(
                    (run.state, next(&run), run.log.qsos().len()),
                    (FdState::AwaitConfirm, roger, 0),
                    "{what}: nothing closed, no 73"
                );
            }
        }
    }

    /// ⭐ FIELD DAY'S RUN CLOSES WITH A CALLER WHOSE CALL IS NONSTANDARD. Running WSJT-X 3.0.2,
    /// such a caller sends its RR73 and 73 with my call hashed and its own whole,
    /// `<K1ABC> W9XYZ/M RR73` (`genStdMsgs`, mainwindow.cpp 9702 and 9710,
    /// `if(bHisCall and !bMyCall)`), and WSJT-X takes that as addressed to itself: its decoder
    /// prints its own call's hash as `<K1ABC>` (`unpack77`, packjt77.f90 644-651,
    /// `n12.eq.hashmy12`), `DecodedText` drops the brackets (decodedtext.cpp 66), and
    /// `auto_sequence` and `processMessage` ask whether that word holds its call (7546, 9108). So
    /// the run logs W9XYZ/M, bare, on its RR73, RRR or 73, and answers the RR73 and RRR with 73
    /// where the caller runs WSJT-X's Field Day activity (9210-9215). Before, it never closed: it
    /// sent the caller its R and exchange again.
    #[test]
    fn field_days_run_closes_with_a_caller_whose_call_is_nonstandard() {
        for me in ["K1ABC", "K1ABC/P"] {
            for (close, answer) in [
                ("RR73", format!("W9XYZ/M <{me}> 73")),
                ("RRR", format!("W9XYZ/M <{me}> 73")),
                ("73", String::new()),
            ] {
                let what = format!("{me}, <{me}> W9XYZ/M {close}");
                let mut run = contest_station(me, "arrlfd", true);
                run.observe(&[dec(&format!("{me} <W9XYZ/M> 1D WI"))], 1);
                assert_eq!(next(&run), format!("<W9XYZ/M> {me} R 2B EMA"), "{what}");
                run.after_tx();
                run.observe(&[dec(&format!("<{me}> W9XYZ/M {close}"))], 3);
                assert_eq!(
                    (run.state, next(&run)),
                    (FdState::Done, answer),
                    "{what}: closed"
                );
                assert_eq!(
                    last_row(&run).0,
                    "W9XYZ/M",
                    "{what}: logged, without the brackets"
                );
                assert_eq!(run.log.qsos().len(), 1, "{what}: once");
            }
        }
        // Winter Field Day's runner, whose caller does not run WSJT-X's activity, sends nothing.
        let session = ContestSession::field_day(FdEvent::WinterFd, "1H", "EMA");
        let mut run = FieldDayStation::running("K1ABC", "FN42", session, "20M");
        run.observe(&[dec("K1ABC <W9XYZ/M> 1H WI")], 1);
        run.after_tx();
        run.observe(&[dec("<K1ABC> W9XYZ/M RR73")], 3);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::Done, String::new()),
            "WFD"
        );
        assert_eq!(last_row(&run).0, "W9XYZ/M", "WFD: logged");
    }

    /// The control: only the caller's own close, to my own call. Hashed to another call, from
    /// another station, or to a call the decoder could not resolve, it closes nothing and the
    /// caller gets the R and exchange again; and the exchanges still take my standard call only
    /// whole (`a_standard_call_goes_out_and_is_heard_exactly_as_before`).
    #[test]
    fn field_days_run_closes_only_on_the_callers_own_close_to_my_call() {
        for (heard, what) in [
            ("<K2DEF> W9XYZ/M RR73", "to another call"),
            ("<K1ABC> N0ABC/M RR73", "from another station"),
            (
                "<...> W9XYZ/M RR73",
                "to a call the decoder could not resolve",
            ),
        ] {
            let mut run = contest_station("K1ABC", "arrlfd", true);
            run.observe(&[dec("K1ABC <W9XYZ/M> 1D WI")], 1);
            run.after_tx();
            run.observe(&[dec(heard)], 3);
            assert_eq!(
                (run.state, next(&run)),
                (
                    FdState::AwaitConfirm,
                    "<W9XYZ/M> K1ABC R 2B EMA".to_string()
                ),
                "{what}: nothing closed"
            );
            assert!(run.log.qsos().is_empty(), "{what}: nothing logged");
        }
    }

    /// ⭐ FIELD DAY'S MESSAGES PASS BY A CALL THE DECODER COULD NOT RESOLVE, as WSJT-X's Field Day
    /// activity does: `processMessage` returns at once for a message that carries `<...>`
    /// (mainwindow.cpp 9097-9099). Running, a caller heard as `<...>` gets no answer and no row,
    /// whether it sends its exchange or rogers first, and the run keeps calling CQ. The same
    /// station's next message, its call resolved, is worked as any other's and logged bare. S&P
    /// does not answer a CQ that names such a call, `CQ FD <...> EN37`, either: WSJT-X puts only a
    /// callsign into its DX call box (`Radio::is_callsign`, 9402), so it builds no message to one.
    #[test]
    fn field_days_run_passes_by_a_caller_the_decoder_could_not_resolve() {
        for heard in ["K1ABC <...> 1D WI", "K1ABC <...> R 1D WI"] {
            let mut run = contest_station("K1ABC", "arrlfd", true);
            run.observe(&[dec(heard)], 1);
            assert_eq!(
                (run.state, next(&run)),
                (FdState::CallingCq, "CQ FD K1ABC FN42".to_string()),
                "{heard}: no answer"
            );
            assert!(run.log.qsos().is_empty(), "{heard}: no row");
            run.observe(&[dec("K1ABC <W9XYZ/M> 1D WI")], 3);
            assert_eq!(
                next(&run),
                "<W9XYZ/M> K1ABC R 2B EMA",
                "{heard}: its next, resolved, is answered"
            );
            run.after_tx();
            run.observe(&[dec("<K1ABC> W9XYZ/M RR73")], 5);
            assert_eq!(next(&run), "W9XYZ/M <K1ABC> 73", "{heard}: and closed");
            assert_eq!(last_row(&run).0, "W9XYZ/M", "{heard}: and logged bare");
        }
        let mut sp = contest_station("K1ABC", "arrlfd", false);
        sp.observe(&[dec("CQ FD <...> EN37")], 0);
        assert_eq!(
            (sp.state, next(&sp)),
            (FdState::Listening, String::new()),
            "S&P: a CQ from <...> is not answered"
        );
        click(&mut sp, "CQ FD W9XYZ EN37");
        assert_eq!(
            next(&sp),
            "W9XYZ K1ABC 2B EMA",
            "S&P: the next CQ, double-clicked, is"
        );
    }

    /// The direction reaches a running station's CQ at once, and every CQ after; S&P calls none.
    #[test]
    fn a_typed_direction_reaches_the_running_cq() {
        let cq = |st: &FieldDayStation| st.pending.as_ref().map(Msg::to_text);
        let mut run = ft8_station("txqp", "FN42", true);
        assert_eq!(cq(&run).as_deref(), Some("CQ K1ABC FN42"));
        run.set_cq_dir("QP");
        assert_eq!(cq(&run).as_deref(), Some("CQ QP K1ABC FN42"));
        run.rearm(true);
        assert_eq!(
            cq(&run).as_deref(),
            Some("CQ QP K1ABC FN42"),
            "after a contact"
        );
        let mut sp = ft8_station("txqp", "FN42", false);
        sp.set_cq_dir("QP");
        assert_eq!(cq(&sp), None, "S&P calls no CQ");
    }

    #[test]
    fn the_standard_messages_answer_only_their_own_contact() {
        let mut sp = ft8_station("tnqp", "FN42", false);
        sp.observe(&[dec("CQ PJ4/K1XYZ")], 1);
        assert_eq!(sp.state, FdState::Listening, "a CQ without a grid");
        click(&mut sp, "CQ W9XYZ EN37");
        sp.observe(&[dec("K1ABC N0ABC -07")], 5);
        assert_eq!(
            sp.state,
            FdState::AwaitExchange,
            "a report from a bystander"
        );
        sp.observe(&[dec("K1ABC W9XYZ -07")], 7);
        sp.observe(&[dec("K1ABC N0ABC RR73")], 9);
        assert_eq!(sp.state, FdState::AwaitConfirm, "an RR73 from a bystander");
        assert!(sp.log.qsos().is_empty());

        let mut run = ft8_station("tnqp", "FN42", true);
        run.observe(&[dec("N0ABC W9XYZ EN37"), dec("N0ABC W9XYZ -07")], 1);
        assert_eq!(run.state, FdState::CallingCq, "sent to somebody else");
        run.observe(&[dec("<N0ABC> PJ4/W9XYZ")], 1);
        assert_eq!(
            run.state,
            FdState::CallingCq,
            "a call with no grid to somebody else"
        );
        run.observe(&[dec("K1ABC W9XYZ EN37")], 3);
        run.observe(&[dec("K1ABC N0ABC R-07")], 5);
        assert_eq!(
            run.state,
            FdState::AwaitExchange,
            "an R-report from a bystander"
        );
        assert!(run.log.qsos().is_empty());
    }

    /// ⭐ The standard messages' run answers a caller's first call that carries no grid, as WSJT-X
    /// running with no activity does. A caller whose call is nonstandard sends it as `t0a` alone,
    /// `<K1ABC> W9XYZ/M` (mainwindow.cpp 9627), and WSJT-X answers a message to it of fewer than
    /// five words with Tx2, the report (`processMessage` 9315-9325), to the call in its DX call box
    /// (9402): `<W9XYZ/M> K1ABC -05`, its call hashed beside mine (`t0a`, 9629-9631, 9655), or
    /// mine hashed beside its where mine is the nonstandard one (`t0b`, 9625-9626). The contact
    /// then runs as any other and is logged without a grid. Not answered, as before: a call to
    /// somebody else, one from `<...>`, and a nonstandard call beside a nonstandard call of mine,
    /// which no message carries with a report.
    #[test]
    fn the_standard_messages_run_answers_a_first_call_with_no_grid() {
        let mut run = ft8_station("txqp", "FN42", true);
        run.observe(&[dec("<K1ABC> W9XYZ/M")], 1);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::AwaitExchange, "<W9XYZ/M> K1ABC -05".to_string()),
            "W9XYZ/M's first call: my report"
        );
        run.after_tx();
        run.observe(&[dec("K1ABC <W9XYZ/M> R-07")], 3);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::Done, "W9XYZ/M <K1ABC> RR73".to_string()),
            "its roger: RR73"
        );
        assert_eq!(last_row(&run).0, "W9XYZ/M", "logged");

        let mut run = ft8_station_as("K1ABC/QRP", "txqp", "FN42", true);
        run.observe(&[dec("<K1ABC/QRP> W9XYZ")], 1);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::AwaitExchange, "W9XYZ <K1ABC/QRP> -05".to_string()),
            "a nonstandard call of mine: my report, my call hashed"
        );
        run.after_tx();
        run.observe(&[dec("<K1ABC/QRP> W9XYZ R-07")], 3);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::Done, "<W9XYZ> K1ABC/QRP RR73".to_string()),
            "and RR73"
        );
        assert_eq!(last_row(&run).0, "W9XYZ", "logged");

        for (mycall, heard, what) in [
            ("K1ABC", "<N0ABC> W9XYZ/M", "a call to somebody else"),
            ("K1ABC", "K1ABC <...>", "a call from <...>"),
            ("K1ABC/QRP", "<K1ABC/QRP> W9XYZ/M", "two nonstandard calls"),
        ] {
            let mut run = ft8_station_as(mycall, "txqp", "FN42", true);
            let cq = next(&run);
            run.observe(&[dec(heard)], 1);
            assert_eq!((run.state, next(&run)), (FdState::CallingCq, cq), "{what}");
        }
    }

    /// A change of messages (a tier change, in the engine) re-arms: a contact half-worked in
    /// one set cannot be finished in another. The same set keeps it.
    #[test]
    fn a_change_of_messages_rearms_and_the_same_set_keeps_the_contact() {
        let mut run = ft8_station("arrlvhf_jan", "FN42", true);
        run.observe(&[dec("K1ABC W9XYZ EN37")], 1);
        run.set_messages(FtMessages::NaVhf, true);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::AwaitConfirm, "W9XYZ K1ABC R FN42".into())
        );
        run.set_messages(FtMessages::FieldDay, true);
        assert_eq!(
            (run.state, next(&run)),
            (FdState::CallingCq, "CQ FD K1ABC FN42".into())
        );
        assert_eq!(run.dxcall, None);

        let mut sp = ft8_station("txqp", "FN42", false);
        sp.observe(&[dec("CQ W9XYZ EN37")], 1);
        sp.set_messages(FtMessages::FieldDay, false);
        assert_eq!((sp.state, next(&sp)), (FdState::Listening, String::new()));
    }

    /// The grid on the air is four characters, as WSJT-X sends it, and in the VHF contests it
    /// is the session's own — the grid a rover moved to.
    #[test]
    fn the_grid_on_the_air_is_four_characters_and_follows_a_move() {
        let run = ft8_station("txqp", "FN42ab", true);
        assert_eq!(next(&run), "CQ K1ABC FN42");
        let mut run = ft8_station("arrlvhf_jan", "FN42ab", true);
        assert_eq!(next(&run), "CQ TEST K1ABC FN42");
        run.log
            .session
            .move_to(&[("GRID", "FN43")])
            .expect("a rover moves");
        run.rearm(true);
        assert_eq!(next(&run), "CQ TEST K1ABC FN43");
        run.observe(&[dec("K1ABC W9XYZ EN37")], 1);
        assert_eq!(next(&run), "W9XYZ K1ABC R FN43");
    }

    // -- §3.3: the sent exchange is a per-ROW value ---------------------------

    /// The log a mobile makes: two rows either side of a move, and the move must not
    /// reach backwards.
    ///
    /// Field Day has no county, so the move here is a SECTION change. That is the same
    /// mechanism (`session.my_exchange` is edited between contacts) on the exchange
    /// this build actually ships, which is the only exchange there is to test it on.
    fn moved_log() -> FieldDayLog {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_000_000));
        // — "I moved": the declared location AND the exchange derived from it, which
        // is one action and not two. The Cabrillo `LOCATION` header follows the
        // session (it is a per-ENTRY value); the QSO lines follow their own rows.
        log.session.my_location.state = "IL".into();
        log.session.my_exchange = vec![
            log.session.exchange.value("CLASS", "3A").unwrap(),
            log.session.exchange.value("SECTION", "IL").unwrap(),
        ];
        assert!(log.log_mode_at("W1AW", "1D", "CT", "CW", 0, 1_782_000_060));
        log
    }

    /// ⭐ **§10's mobile-history test.** Log either side of a move, export, and the
    /// first row still carries what it actually sent.
    #[test]
    fn moving_the_session_does_not_relabel_a_row_already_logged() {
        let log = moved_log();
        let cbr = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let lines: Vec<&str> = cbr.lines().filter(|l| l.starts_with("QSO:")).collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].contains("W9XYZ 3A WI K1ABC 2A EMA"),
            "row 1 was relabelled by a move that happened after it: {}",
            lines[0]
        );
        assert!(
            lines[1].contains("W9XYZ 3A IL W1AW 1D CT"),
            "row 2 does not carry the exchange it sent: {}",
            lines[1]
        );
        // The header is the ENTRY's declared location — the session's, deliberately
        // not row 1's. Cabrillo puts it in a header, once, for exactly that reason.
        assert!(cbr.contains("\nLOCATION: IL\n"), "{cbr}");
    }

    /// POSITIVE CONTROL for the test above, and it is the whole reason that green is
    /// evidence. The same fixture written the way `cabrillo()` used to write it — the
    /// LOG's one sent exchange on every line — MUST produce different bytes. If it does
    /// not, the assertions pass because the two values happen to agree rather than
    /// because the writer reads the row.
    #[test]
    fn the_mobile_history_assertion_discriminates() {
        let log = moved_log();
        let from_the_row = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let from_the_session: String = from_the_row
            .lines()
            .map(|l| {
                if l.starts_with("QSO:") {
                    // The defect, reconstructed: every line takes the session's
                    // CURRENT sent exchange instead of its own row's.
                    l.replace("W9XYZ 3A WI", "W9XYZ 3A IL")
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(
            from_the_row.trim_end(),
            from_the_session.trim_end(),
            "the byte comparison cannot see a relabelled row"
        );
    }

    /// ⭐ **A moved sent exchange survives a restart**, which is what makes the test
    /// above hold across the journal rather than only in memory.
    ///
    /// The pre-move row's `tx` differs from the session's current one, so the writer
    /// stamps it with `APP_NEXUS_MYEX`; the post-move row matches and falls back. Both
    /// come back, which is why the exported bytes are unchanged.
    #[test]
    fn a_moved_sent_exchange_survives_the_journal() {
        let log = moved_log();
        let journal = log.adif();
        assert!(
            journal.contains("APP_NEXUS_MYEX"),
            "the row that no longer matches the session was journaled with no sent side"
        );
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "IL"),
            "20m",
        );
        restored.merge_adif(&journal, 0);
        assert_eq!(restored.qso_count(), 2);
        assert_eq!(
            restored
                .cabrillo(14_074)
                .expect("a single-mode event exports one entry"),
            log.cabrillo(14_074)
                .expect("a single-mode event exports one entry")
        );
        // POSITIVE CONTROL: strip the carrier and the pre-move row falls back to the
        // session's current exchange, which is the WRONG section — so the tag is what
        // is carrying the answer, not the fallback happening to be right.
        let stripped: String = journal
            .lines()
            .map(|l| match l.find("<APP_NEXUS_MYEX:") {
                Some(i) => format!("{}{}", &l[..i], &l[l.rfind("<EOR>").unwrap_or(l.len())..]),
                None => l.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut blind = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "IL"),
            "20m",
        );
        blind.merge_adif(&stripped, 0);
        assert_ne!(
            blind
                .cabrillo(14_074)
                .expect("a single-mode event exports one entry"),
            log.cabrillo(14_074)
                .expect("a single-mode event exports one entry"),
            "APP_NEXUS_MYEX is not what restores the pre-move row"
        );
    }

    /// ⭐ **A section changed in Settings re-labels no row already logged.** Settings does not
    /// move the running session (only I moved does), so a log whose rows all matched it wrote
    /// no sent side, and the session the next entry built from Settings read every row back
    /// with the new section.
    #[test]
    fn a_section_changed_in_settings_relabels_no_row_already_logged() {
        let mut wi = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(wi.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_000_000));
        assert!(wi.log_mode_at("W1AW", "1D", "CT", "CW", 0, 1_782_000_060));
        let mut il = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "IL"),
            "20m",
        );
        il.merge_adif(&wi.journal_adif(), 0);
        let sent: Vec<&str> = il.qsos().iter().map(|q| q.sent("SECTION")).collect();
        assert_eq!(
            sent,
            ["WI", "WI"],
            "each row keeps the section it was sent from"
        );
        let lines = |log: &FieldDayLog| -> Vec<String> {
            log.cabrillo(14_074)
                .expect("a single-mode event exports one entry")
                .lines()
                .filter(|l| l.starts_with("QSO:"))
                .map(str::to_string)
                .collect()
        };
        assert_eq!(lines(&il), lines(&wi), "and the Cabrillo file says so");
    }

    /// §4, direction 2: being the mobile is what `by_sent_fields` is for, and Field
    /// Day declares neither list — so the same station on the same band and mode is a
    /// dupe whatever the session is sending, and the key is exactly the shipped tuple.
    #[test]
    fn field_days_key_ignores_the_sent_side_because_its_rule_names_no_slots() {
        let mut log = moved_log();
        let rule = log.dupe_rule();
        assert_eq!(rule.by_fields, &[] as &[&str]);
        assert_eq!(rule.by_sent_fields, &[] as &[&str]);
        assert!(
            logged_dupe(
                log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_000_120),
                &log
            ),
            "a move must not un-dupe a station under a rule that names no sent slot"
        );
    }

    /// The Illinois QSO Party from Kane County: an exchange that SENDS a report (RST and
    /// county), where Field Day's sends none.
    fn ilqp_from_kane() -> FieldDayLog {
        let rs = crate::fd_rules::ruleset_by_id("ilqp", crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let data = crate::contest::StationData {
            mycall: "W9XYZ".into(),
            contest_qth_state: "IL".into(),
            contest_qth_county: "KANE".into(),
            ..Default::default()
        };
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        FieldDayLog::new("W9XYZ", session, "20m")
    }

    /// One ILQP contact in `mode`: the report and county `call` sent me.
    fn ilqp_contact(log: &mut FieldDayLog, call: &str, rst: &str, county: &str, mode: &str) {
        let rx = [
            ("RST".to_string(), rst.to_string()),
            ("QTH".to_string(), county.to_string()),
        ];
        let when = 1_792_000_000 + 60 * log.qsos().len() as u64;
        assert!(log.log_fields_at(call, &rx, mode, "", 0, when), "{call}");
    }

    /// What `call`'s row sent as its report, set on the row: no session or other row holds it.
    fn set_sent_rst(log: &mut FieldDayLog, call: &str, rst: &str) {
        let row = log
            .qsos_mut()
            .iter_mut()
            .find(|q| q.call == call)
            .expect("the row");
        row.tx
            .iter_mut()
            .find(|v| v.key == "RST")
            .expect("an ILQP row sends a report")
            .raw = rst.to_string();
    }

    /// `call`'s record in an ADIF file.
    fn record_for<'a>(adif: &'a str, call: &str) -> &'a str {
        let tag = format!("<CALL:{}>{call} ", call.len());
        adif.split("<EOR>")
            .find(|r| r.contains(&tag))
            .unwrap_or_else(|| panic!("no record for {call} in\n{adif}"))
    }

    /// ⭐ **The submitted ADIF writes each row's OWN sent report as `RST_SENT`**, the tag
    /// another program reads. The report rode only in the private `APP_NEXUS_MYEX` carrier, so
    /// a file imported elsewhere had none, while the merged Logbook's ADIF had it.
    ///
    /// By value, on data that disagrees wherever a wrong source could pass: the two rows sent
    /// different reports, neither is the session's `599`, and each received another value.
    #[test]
    fn the_submitted_adif_writes_each_rows_own_sent_report() {
        let mut log = ilqp_from_kane();
        ilqp_contact(&mut log, "K9AAA", "559", "COOK", "CW");
        ilqp_contact(&mut log, "K9BBB", "579", "LAKE", "CW");
        set_sent_rst(&mut log, "K9AAA", "449");
        set_sent_rst(&mut log, "K9BBB", "339");
        let adif = log.submission_adif();
        for (call, want) in [("K9AAA", "<RST_SENT:3>449 "), ("K9BBB", "<RST_SENT:3>339 ")] {
            let r = record_for(&adif, call);
            assert!(r.contains(want), "{call} does not carry {want:?}: {r}");
            assert_eq!(r.matches("<RST_SENT:").count(), 1, "{call}: {r}");
        }
    }

    /// ⭐ **…and nothing else in the file moves**: a whole submitted file, byte for byte, which is
    /// the file this build wrote before with the one tag added, and its QSO number as `STX`
    /// ([`FieldDayLog::qso_numbers`]). A phone contact, so the row's `59` is neither the
    /// session's `599` nor the `57` it received, and the carrier is written beside it as before.
    #[test]
    fn a_submitted_record_gains_its_sent_report_and_nothing_else() {
        let mut log = ilqp_from_kane();
        log.dial_khz = 14_250;
        log.on_air_hz = 14_250_000;
        ilqp_contact(&mut log, "K9AAA", "57", "COOK", "PH");
        assert_eq!(
            log.submission_adif(),
            "ADIF Export from Nexus\n<PROGRAMID:5>Nexus\n<EOH>\n\
             <CALL:5>K9AAA <MODE:3>SSB <BAND:3>20m <FREQ:9>14.250000 \
             <APP_NEXUS_DIALKHZ:5>14250 <QSO_DATE:8>20261014 <TIME_ON:6>174640 \
             <CONTEST_ID:12>IL QSO Party <RST_RCVD:2>57 <RST_SENT:2>59 <STX:1>1 \
             <APP_NEXUS_EX:28>RST::57;QTH:il_counties:COOK \
             <APP_NEXUS_MYEX:28>RST::59;QTH:il_counties:KANE <APP_NEXUS_QSEQ:1>1 <EOR>\n"
        );
    }

    /// A row that sent no report writes no `RST_SENT`, never the session's `599` in its place, and
    /// an exchange that declares no report (Field Day) writes none on any row. No shipped path
    /// logs a QSO-party row without a report, so this one is built by hand.
    #[test]
    fn a_row_without_a_sent_report_writes_no_rst_sent() {
        let mut log = ilqp_from_kane();
        ilqp_contact(&mut log, "K9AAA", "559", "COOK", "CW");
        ilqp_contact(&mut log, "K9BBB", "579", "LAKE", "CW");
        set_sent_rst(&mut log, "K9AAA", "");
        let adif = log.submission_adif();
        // The control first: the row beside it sent the session's report, and writes it.
        let with = record_for(&adif, "K9BBB");
        assert!(with.contains("<RST_SENT:3>599 "), "the control: {with}");
        let without = record_for(&adif, "K9AAA");
        assert!(!without.contains("<RST_SENT:"), "{without}");
        let field_day = moved_log().submission_adif();
        assert!(!field_day.contains("<RST_SENT:"), "{field_day}");
    }

    /// ⭐ **The journal keeps its bytes, and the club's export gains the sent report.** One log,
    /// two whole files: `journal_adif` writes no `RST_SENT`, as before, because a restore reads a
    /// row's sent side from `APP_NEXUS_MYEX`; `adif`, the export a club's file is written from,
    /// is the same bytes with the one tag added. The phone row's `59` is neither the session's
    /// `599` nor the `57` it received, so a journal that wrote the tag would show it here.
    #[test]
    fn the_journal_keeps_its_bytes_where_the_club_export_gains_the_sent_report() {
        let mut log = ilqp_from_kane();
        log.dial_khz = 14_250;
        log.on_air_hz = 14_250_000;
        ilqp_contact(&mut log, "K9AAA", "57", "COOK", "PH");
        let journal = "ADIF Export from Nexus\n<PROGRAMID:5>Nexus\n<EOH>\n\
             <CALL:5>K9AAA <MODE:3>SSB <BAND:3>20m <FREQ:9>14.250000 \
             <APP_NEXUS_DIALKHZ:5>14250 <QSO_DATE:8>20261014 <TIME_ON:6>174640 \
             <CONTEST_ID:12>IL QSO Party <RST_RCVD:2>57 \
             <APP_NEXUS_EX:28>RST::57;QTH:il_counties:COOK \
             <APP_NEXUS_MYEX:28>RST::59;QTH:il_counties:KANE <APP_NEXUS_QSEQ:1>1 <EOR>\n";
        assert_eq!(log.journal_adif(), journal);
        assert_eq!(
            log.adif(),
            journal.replace("<RST_RCVD:2>57 ", "<RST_RCVD:2>57 <RST_SENT:2>59 ")
        );
    }

    /// ⭐ **The journal keeps the call each contact was logged on, and no export carries it.** A
    /// restart reads every row into a session opened under whatever call Settings holds then, so
    /// a contact logged on the club's call and one logged on the operator's own after it must each
    /// come back with its own ([`LoggedQso::on_air_call`]). The submitted file and the club's
    /// export are as they were, and a row journaled before the tag existed carries none.
    #[test]
    fn the_journal_keeps_the_call_each_contact_was_logged_on() {
        let mut log = ilqp_from_kane();
        log.on_air_call = "W9XYZ".into();
        ilqp_contact(&mut log, "K9AAA", "599", "COOK", "CW");
        log.on_air_call = "K9OWN".into();
        ilqp_contact(&mut log, "K9BBB", "599", "WILL", "CW");
        let journal = log.journal_adif();
        assert!(
            record_for(&journal, "K9AAA").contains("<APP_NEXUS_ONAIR:5>W9XYZ "),
            "{journal}"
        );
        assert!(
            record_for(&journal, "K9BBB").contains("<APP_NEXUS_ONAIR:5>K9OWN "),
            "{journal}"
        );
        for export in [log.adif(), log.submission_adif()] {
            assert!(!export.contains("APP_NEXUS_ONAIR"), "{export}");
        }
        let calls = |log: &FieldDayLog| -> Vec<(String, String)> {
            log.qsos()
                .iter()
                .map(|q| (q.call.clone(), q.on_air_call.clone()))
                .collect()
        };
        // Read back by a session opened under a third call: each row keeps its own.
        let mut back = ilqp_from_kane();
        back.mycall = "N9NEW".into();
        back.on_air_call = "N9NEW".into();
        back.merge_adif(&journal, 0);
        let owned =
            |pairs: [(&str, &str); 2]| pairs.map(|(a, b)| (a.to_string(), b.to_string())).to_vec();
        assert_eq!(
            calls(&back),
            owned([("K9AAA", "W9XYZ"), ("K9BBB", "K9OWN")])
        );
        // A journal written before the tag: its rows carry none.
        let older = journal
            .replace("<APP_NEXUS_ONAIR:5>W9XYZ ", "")
            .replace("<APP_NEXUS_ONAIR:5>K9OWN ", "");
        let mut old = ilqp_from_kane();
        old.merge_adif(&older, 0);
        assert_eq!(calls(&old), owned([("K9AAA", ""), ("K9BBB", "")]));
    }

    /// **Each row keeps the radio and the power it was made with**, stamped at log time
    /// from the log's RIGHT NOW values, through the journal and a restart; no contest file carries
    /// them; and a journal written before they were recorded still reads, its rows carrying none.
    #[test]
    fn the_journal_keeps_the_radio_and_power_each_contact_was_made_with() {
        let mut log = ilqp_from_kane();
        log.rig = "Icom IC-7300".into();
        log.tx_power = "100".into();
        ilqp_contact(&mut log, "K9AAA", "599", "COOK", "CW");
        log.rig = "Elecraft KX3".into();
        log.tx_power = "5".into();
        ilqp_contact(&mut log, "K9BBB", "599", "WILL", "CW");
        let journal = log.journal_adif();
        assert!(
            record_for(&journal, "K9AAA").contains("<MY_RIG:12>Icom IC-7300 <TX_PWR:3>100 "),
            "{journal}"
        );
        assert!(
            record_for(&journal, "K9BBB").contains("<MY_RIG:12>Elecraft KX3 <TX_PWR:1>5 "),
            "{journal}"
        );
        for export in [log.adif(), log.submission_adif()] {
            assert!(
                !export.contains("MY_RIG") && !export.contains("TX_PWR"),
                "{export}"
            );
        }
        let kit = |log: &FieldDayLog| -> Vec<(String, String, String)> {
            log.qsos()
                .iter()
                .map(|q| (q.call.clone(), q.rig.clone(), q.tx_power.clone()))
                .collect()
        };
        let owned = |rows: [(&str, &str, &str); 2]| {
            rows.map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
                .to_vec()
        };
        let mut back = ilqp_from_kane();
        back.merge_adif(&journal, 0);
        assert_eq!(
            kit(&back),
            owned([
                ("K9AAA", "Icom IC-7300", "100"),
                ("K9BBB", "Elecraft KX3", "5")
            ])
        );
        let older = journal
            .replace("<MY_RIG:12>Icom IC-7300 <TX_PWR:3>100 ", "")
            .replace("<MY_RIG:12>Elecraft KX3 <TX_PWR:1>5 ", "");
        assert!(!older.contains("MY_RIG"), "the control: {older}");
        let mut old = ilqp_from_kane();
        old.merge_adif(&older, 0);
        assert_eq!(kit(&old), owned([("K9AAA", "", ""), ("K9BBB", "", "")]));
    }

    /// **Each row keeps the grid it was made from**, stamped at log time from the log's RIGHT
    /// NOW grid, through the journal and a restart, under a private tag no contest file writes;
    /// and a journal written before it was recorded still reads, its rows carrying none.
    #[test]
    fn the_journal_keeps_the_grid_each_contact_was_made_from() {
        let mut log = ilqp_from_kane();
        log.grid = "EN52".into();
        ilqp_contact(&mut log, "K9AAA", "599", "COOK", "CW");
        log.grid = "EN61AB".into();
        ilqp_contact(&mut log, "K9BBB", "599", "WILL", "CW");
        let journal = log.journal_adif();
        assert!(
            record_for(&journal, "K9AAA").contains("<APP_NEXUS_MYGRID:4>EN52 "),
            "{journal}"
        );
        assert!(
            record_for(&journal, "K9BBB").contains("<APP_NEXUS_MYGRID:6>EN61AB "),
            "{journal}"
        );
        for export in [log.adif(), log.submission_adif()] {
            assert!(
                !export.contains("APP_NEXUS_MYGRID") && !export.contains("MY_GRIDSQUARE"),
                "{export}"
            );
        }
        let grids = |log: &FieldDayLog| -> Vec<(String, String)> {
            log.qsos()
                .iter()
                .map(|q| (q.call.clone(), q.grid.clone()))
                .collect()
        };
        let owned =
            |rows: [(&str, &str); 2]| rows.map(|(a, b)| (a.to_string(), b.to_string())).to_vec();
        let mut back = ilqp_from_kane();
        back.merge_adif(&journal, 0);
        assert_eq!(
            grids(&back),
            owned([("K9AAA", "EN52"), ("K9BBB", "EN61AB")])
        );
        let older = journal
            .replace("<APP_NEXUS_MYGRID:4>EN52 ", "")
            .replace("<APP_NEXUS_MYGRID:6>EN61AB ", "");
        assert!(!older.contains("APP_NEXUS_MYGRID"), "the control: {older}");
        let mut old = ilqp_from_kane();
        old.merge_adif(&older, 0);
        assert_eq!(grids(&old), owned([("K9AAA", ""), ("K9BBB", "")]));
    }
}

#[cfg(test)]
mod cabrillo_header_tests {
    use super::*;
    use crate::contest::{ContestSession, OperatorCategory};

    fn one_qso_log(entry: OperatorCategory) -> FieldDayLog {
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.entry_category = entry;
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        log
    }

    /// ⭐ THE SHIPPED BUG. `cabrillo()` wrote `CATEGORY-OPERATOR: MULTI-OP` as a
    /// string literal, so every solo Field Day entry Nexus has ever exported claimed
    /// more than one operator was at the station — in the file ARRL scores.
    #[test]
    fn a_single_operator_entry_declares_single_op() {
        let cab = one_qso_log(OperatorCategory::SingleOp)
            .cabrillo(14_074)
            .expect("a single-mode event exports");
        assert!(
            cab.contains("CATEGORY-OPERATOR: SINGLE-OP\n"),
            "a single-op entry still submits a MULTI-OP header:\n{cab}"
        );
        assert!(
            !cab.contains("MULTI-OP"),
            "the literal survived somewhere:\n{cab}"
        );
    }

    /// POSITIVE CONTROL: a club entry still declares `MULTI-OP`, so the test above is
    /// the declaration being read and not the token being renamed.
    #[test]
    fn a_club_entry_still_declares_multi_op() {
        let cab = one_qso_log(OperatorCategory::MultiOp)
            .cabrillo(14_074)
            .expect("a single-mode event exports");
        assert!(cab.contains("CATEGORY-OPERATOR: MULTI-OP\n"), "{cab}");
    }

    /// ⭐ §6.2 — **both callsign columns are on the line, and Sweepstakes gets
    /// exactly two, not four.**
    ///
    /// The template a previous draft carried was exchange slots only, which loses
    /// both callsigns on every contest and produces an unsubmittable line. Field Day's
    /// golden pins the structural columns; this pins the derived exception — a side
    /// whose slot list declares a `Call` slot does not write that slot a SECOND time
    /// among the exchange columns, because the structural column already is it.
    ///
    /// ⚠️ **The expected line is ARRL's own published template, and it corrects §6.2.**
    /// The design sketch assumed the opposite exception (structural column suppressed,
    /// call third of five) and explicitly reserved the question — *"the one thing this
    /// walk does not settle is the column order, and it must not"* — for the batch that
    /// read the sponsor. <https://www.arrl.org/cabrillo-format-tutorial>, ARRL's own
    /// Cabrillo Format & Tutorial page, read 2026-09-09, publishes a **QSO DATA
    /// TEMPLATE** for Sweepstakes with a lettered legend:
    ///
    /// ```text
    /// QSO: 14000 CW 2009-11-07 2100 W1AW   1 M 38 CT K8MM   1 Q 92 MI
    /// ```
    /// > *"E= Your call. F= Your QSO #. G= Your precedence. H= Your check (the last two
    /// > numbers of the year you were first licensed). I= Your ARRL Section. J= The call
    /// > of the station you worked. K= Their QSO number to you. L= Their precedence.
    /// > M= Their check. N = Their ARRL Section."*
    ///
    /// The callsign is column **E** — first of the side, the structural position every
    /// other contest uses — and serial, precedence, check and section follow it with no
    /// callsign among them. The line below is that shape, contact for contact.
    #[test]
    fn a_sweepstakes_line_carries_each_callsign_exactly_once() {
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.exchange = crate::contest::exchanges::sweepstakes_shaped();
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        let spec = log.session.exchange;
        let v = |key: &'static str, raw: &str| crate::contest::FieldValue {
            key: spec.field(key).expect("declared slot").key,
            raw: raw.to_string(),
            domain: None,
        };
        // "1 A W9XYZ 74 WI" sent; K2DEF answers with "12 A K2DEF 71 CT".
        let tx = ["1", "A", "W9XYZ", "74", "WI"];
        let rx = ["12", "A", "K2DEF", "71", "CT"];
        let slots = ["NR", "PREC", "CALL", "CK", "SEC"];
        assert!(log.log_exchange_at(
            "K2DEF",
            slots.iter().zip(rx).map(|(k, r)| v(k, r)).collect(),
            slots.iter().zip(tx).map(|(k, r)| v(k, r)).collect(),
            "CW",
            "",
            0,
            1_782_583_500,
        ));
        let cab = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let line = cab
            .lines()
            .find(|l| l.starts_with("QSO:"))
            .expect("one QSO line");
        assert_eq!(
            line, "QSO: 14000 CW 2026-06-27 1805 W9XYZ 1 A 74 WI K2DEF 12 A 71 CT",
            "ARRL's own template: <your call> <NR> <PREC> <CK> <SEC> <their call> \
             <NR> <PREC> <CK> <SEC> — the CALL slot is the structural column and is \
             not repeated inside the exchange"
        );
        // §6.2's own control, in its own words: exactly two callsign occurrences.
        assert_eq!(line.matches("W9XYZ").count(), 1, "{line}");
        assert_eq!(line.matches("K2DEF").count(), 1, "{line}");
    }

    /// POSITIVE CONTROL for the line above: Field Day declares no `Call` slot, so
    /// both structural columns ARE written — a writer that suppressed them for
    /// everyone would pass the "exactly once" assertions above by losing them.
    #[test]
    fn a_field_day_line_still_carries_both_structural_callsigns() {
        let cab = one_qso_log(OperatorCategory::SingleOp)
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let line = cab
            .lines()
            .find(|l| l.starts_with("QSO:"))
            .expect("one QSO line");
        assert_eq!(
            line, "QSO: 14000 CW 2026-06-27 1805 W9XYZ 3A WI K1ABC 2A EMA",
            "my call, my exchange, their call, their exchange"
        );
    }

    /// The trailing transmitter column, and its absence. Neither Field Day sponsor's
    /// template has one, so a Field Day line must not grow a column; a session that
    /// declares one gets it, last.
    #[test]
    fn the_transmitter_column_rides_only_where_a_template_declares_it() {
        let mut log = one_qso_log(OperatorCategory::SingleOp);
        let without = log.cabrillo(14_074).expect("exports");
        assert!(
            without.ends_with("K1ABC 2A EMA\nEND-OF-LOG:\n"),
            "Field Day's line ends with their exchange:\n{without}"
        );
        log.session.transmitter_id = Some(1);
        let with = log.cabrillo(14_074).expect("exports");
        assert!(
            with.ends_with("K1ABC 2A EMA 1\nEND-OF-LOG:\n"),
            "the transmitter id is the LAST column:\n{with}"
        );
    }

    /// …and a checklog, which is neither.
    #[test]
    fn a_checklog_entry_declares_checklog() {
        let cab = one_qso_log(OperatorCategory::Checklog)
            .cabrillo(14_074)
            .expect("a single-mode event exports");
        assert!(cab.contains("CATEGORY-OPERATOR: CHECKLOG\n"), "{cab}");
    }

    /// ⭐ **No value can open a line of either file this log writes** — the writers' own
    /// guard, under club sync's boundary (which keeps out any contact that would need it): a
    /// line feed, a carriage return or another control character inside a call, an exchange
    /// value either way, a band or an operator is written as a space, so every line of the
    /// Cabrillo and every record of the ADIF is one the writer opened. Winter Field Day,
    /// whose `X-EXCHANGE` and `OPERATORS` headers are read off the rows, so headers are under
    /// test too. A log holding none writes the bytes it always did (the §8(a) goldens).
    #[test]
    fn no_value_can_open_a_line_of_the_cabrillo_or_the_adif() {
        let breaker = "\nQSO: 7000 CW 2027-01-23 1702 W9XYZ 3O WI K1FAK 1O CT\r\u{1b}\u{2028}";
        let b = |v: &str| format!("{v}{breaker}");
        let session = ContestSession::field_day(FdEvent::WinterFd, "3O", "WI");
        let spec = session.exchange;
        let fields = |class: &str, section: &str| -> Vec<crate::contest::FieldValue> {
            [("CLASS", class), ("SECTION", section)]
                .iter()
                .filter_map(|(k, v)| spec.value(k, v))
                .collect()
        };
        let mut log = FieldDayLog::new("W9XYZ", session, &b("40m"));
        log.operator = b("K9OP");
        assert!(log.log_exchange_at(
            &b("K1ABC"),
            fields(&b("2O"), &b("EMA")),
            fields(&b("3O"), &b("WI")),
            "CW",
            "",
            0,
            1_800_000_000,
        ));
        let cab = log
            .cabrillo_with(0, &crate::contest::CabrilloEntrant::default())
            .unwrap();
        let adif = log.adif();
        let stray = |s: &str| {
            s.chars()
                .filter(|c| (c.is_control() && *c != '\n') || matches!(c, '\u{2028}' | '\u{2029}'))
                .count()
        };
        assert_eq!((stray(&cab), stray(&adif)), (0, 0), "{cab}\n{adif}");
        assert_eq!(
            cab.lines().filter(|l| l.starts_with("QSO:")).count(),
            1,
            "{cab}"
        );
        assert!(
            cab.lines()
                .any(|l| l.starts_with("X-EXCHANGE: 3O ") && l.contains("K1FAK")),
            "the sent class reaches a header, on its one line: {cab}"
        );
        assert!(
            cab.lines()
                .any(|l| l.starts_with("OPERATORS: K9OP ") && l.contains("K1FAK")),
            "the operator reaches a header, on its one line: {cab}"
        );
        let records: Vec<&str> = adif.split_once("<EOH>\n").unwrap().1.lines().collect();
        assert_eq!(records.len(), 1, "{adif}");
        assert!(records[0].ends_with("<EOR>"), "{adif}");
    }
}

#[cfg(test)]
mod edit_tests {
    //! Editing a contact already in the log ([`FieldDayLog::edit_contact`]), by value: the marks
    //! and the score an edit moves, the numbers it keeps, and what the journal reads back.

    use super::*;
    use crate::contest::{ContestSession, StationData};

    const T0: u64 = 1_792_000_000;

    fn fd() -> FieldDayLog {
        FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        )
    }

    fn contest(event: &str, state: &str, county: &str) -> FieldDayLog {
        let rs = crate::fd_rules::ruleset_by_id(event, crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let data = StationData {
            mycall: "W9XYZ".into(),
            mygrid: "EN52".into(),
            contest_qth_state: state.into(),
            contest_qth_county: county.into(),
            ..Default::default()
        };
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        FieldDayLog::new("W9XYZ", session, "20m")
    }

    /// ARRL Sweepstakes CW from Wisconsin: a serial on every contact, the section the
    /// multiplier, and a station worked once whatever the band.
    fn ss() -> FieldDayLog {
        let rs = crate::fd_rules::ruleset_by_id("arrlss_cw", crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let data = StationData {
            mycall: "W9XYZ".into(),
            fd_section: "WI".into(),
            contest_check: "99".into(),
            contest_category_power: "LOW".into(),
            contest_category_assisted: "NON-ASSISTED".into(),
            ..Default::default()
        };
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        FieldDayLog::new("W9XYZ", session, "20m")
    }

    /// What a Sweepstakes station sends: its serial, A, its call, its check and section.
    fn ss_rx<'a>(nr: &'a str, call: &'a str, sec: &'a str) -> [(&'a str, &'a str); 5] {
        [
            ("NR", nr),
            ("PREC", "A"),
            ("CALL", call),
            ("CK", "99"),
            ("SEC", sec),
        ]
    }

    fn pairs(fields: &[(&str, &str)]) -> Vec<(String, String)> {
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// One contact as the strip logs it: the peer bound to its serial, the row, the binding let go.
    fn log(l: &mut FieldDayLog, call: &str, fields: &[(&str, &str)], when: u64) -> bool {
        l.session.working(call, when);
        let logged = l.log_fields_at(call, &pairs(fields), "CW", "", 0, when);
        l.session.logged(call);
        logged
    }

    fn edit(call: &str, rows: &[&[(&str, &str)]], band: &str, mode: &str) -> ContactEdit {
        ContactEdit {
            call: call.into(),
            rows: rows.iter().map(|r| pairs(r)).collect(),
            band: band.into(),
            mode: mode.into(),
        }
    }

    fn calls(l: &FieldDayLog) -> Vec<&str> {
        l.qsos().iter().map(|q| q.call.as_str()).collect()
    }

    fn dupes(l: &FieldDayLog) -> Vec<bool> {
        l.qsos().iter().map(|q| q.dupe).collect()
    }

    fn mults(l: &FieldDayLog) -> Option<u32> {
        l.ruleset().scoring.score(l.score_rows(), 1).2
    }

    const IL: &[(&str, &str)] = &[("CLASS", "3A"), ("SECTION", "IL")];

    /// ⭐ **A busted call edited into the station it was becomes the duplicate it was** — logged,
    /// marked, worth nothing — and keeps its time and its number: nothing is spent.
    #[test]
    fn a_busted_call_edited_into_a_worked_station_is_marked_the_dupe_it_was() {
        let mut l = fd();
        assert!(log(&mut l, "K2DEF", IL, T0));
        assert!(log(&mut l, "K2DEG", IL, T0 + 60));
        assert_eq!(
            (dupes(&l), l.qso_count(), l.qso_points()),
            (vec![false, false], 2, 4),
            "premise: two CW contacts"
        );
        let was = l
            .edit_contact("k2deg", T0 + 60, &edit("K2DEF", &[IL], "20m", ""))
            .expect("the contact is there");
        assert_eq!(was[0].call, "K2DEG", "the answer is the row as it was");
        assert_eq!(calls(&l), ["K2DEF", "K2DEF"]);
        assert_eq!(
            (dupes(&l), l.qso_count(), l.qso_points()),
            (vec![false, true], 1, 2),
            "the edited row is the dupe, worth nothing"
        );
        let numbers: Vec<(u64, u64)> = l.qsos().iter().map(|q| (q.seq, q.when_unix)).collect();
        assert_eq!(
            numbers,
            [(1, T0), (2, T0 + 60)],
            "the same numbers and times"
        );
        assert_eq!(l.max_seq(), 2, "no number spent");
    }

    /// ⭐ **An edit moves the mark on a LATER row it did not touch**: the busted first contact,
    /// edited into the station worked after it, makes that later row the duplicate — and edited
    /// into another station, makes it a contact again.
    #[test]
    fn an_edit_re_marks_every_later_row_it_affects() {
        let mut l = fd();
        assert!(log(&mut l, "K2DEG", IL, T0));
        assert!(log(&mut l, "K2DEF", IL, T0 + 60));
        assert_eq!(dupes(&l), [false, false], "premise");
        l.edit_contact("K2DEG", T0, &edit("K2DEF", &[IL], "20m", ""))
            .expect("edited");
        assert_eq!(dupes(&l), [false, true], "the later row is the dupe now");
        let mn = &[("CLASS", "1D"), ("SECTION", "MN")];
        l.edit_contact("K2DEF", T0, &edit("N0ABC", &[mn], "20m", ""))
            .expect("edited again");
        assert_eq!(calls(&l), ["N0ABC", "K2DEF"]);
        assert_eq!(dupes(&l), [false, false], "…and a contact again");
        assert_eq!(l.worked_sections(), ["IL", "MN"]);
    }

    /// ⭐ **A dupe edited into the station it really was is a new multiplier** — Sweepstakes,
    /// whose multiplier is the section — and the serial it was sent keeps its number, as does
    /// the serial the next contact gets. The prefix is resolved again from the corrected call.
    #[test]
    fn a_dupe_edited_into_a_new_station_is_a_new_multiplier_and_keeps_its_serial() {
        let mut l = ss();
        assert!(log(&mut l, "K1ABC", &ss_rx("12", "K1ABC", "EMA"), T0));
        assert!(log(&mut l, "K1ABC", &ss_rx("13", "K1ABC", "EMA"), T0 + 60));
        assert_eq!(
            (dupes(&l), mults(&l)),
            (vec![false, true], Some(1)),
            "premise"
        );
        let sent = |l: &FieldDayLog| -> Vec<String> {
            l.qsos().iter().map(|q| q.sent("NR").to_string()).collect()
        };
        let (serials, next) = (sent(&l), l.session.next_serial);
        assert_eq!(serials.len(), 2, "premise: each row carries its serial");
        assert_ne!(serials[0], serials[1], "premise: two serials");
        l.edit_contact(
            "K1ABC",
            T0 + 60,
            &edit("W9AA", &[&ss_rx("13", "W9AA", "WI")], "20m", ""),
        )
        .expect("edited");
        assert_eq!(calls(&l), ["K1ABC", "W9AA"]);
        assert_eq!(
            (dupes(&l), mults(&l)),
            (vec![false, false], Some(2)),
            "a contact again, and a new section"
        );
        assert_eq!(l.qsos()[1].prefix.as_deref(), Some("W9"));
        assert_eq!(sent(&l), serials, "each row keeps the serial it was sent");
        assert_eq!(
            l.session.next_serial, next,
            "and the next contact's is unchanged"
        );
    }

    /// ⭐ **A county line is one contact**: an edit takes one vector per county in row order, every
    /// row takes the new call, and a line edited into another number of counties is refused.
    #[test]
    fn a_county_line_is_edited_as_one_contact() {
        let mut l = contest("ilqp", "IL", "KANE");
        l.session.working("K9AAA", T0);
        for county in ["COOK", "LAKE"] {
            assert!(l.log_fields_at(
                "K9AAA",
                &pairs(&[("RST", "599"), ("QTH", county)]),
                "CW",
                "",
                0,
                T0
            ));
        }
        l.session.logged("K9AAA");
        let was = l.qsos().to_vec();
        assert_eq!(
            l.edit_contact(
                "K9AAA",
                T0,
                &edit("K9AAB", &[&[("RST", "599"), ("QTH", "COOK")]], "20m", "")
            ),
            Err(EditRefusal::Rows)
        );
        assert_eq!(l.qsos(), &was[..], "a refusal changes nothing");
        let line: &[&[(&str, &str)]] = &[
            &[("RST", "599"), ("QTH", "COOK")],
            &[("RST", "599"), ("QTH", "DUPG")],
        ];
        let before = l
            .edit_contact("K9AAA", T0, &edit("K9AAB", line, "20m", ""))
            .expect("edited");
        assert_eq!(before.len(), 2, "both rows, as they were");
        let rows: Vec<(&str, &str)> = l
            .qsos()
            .iter()
            .map(|q| (q.call.as_str(), q.rcvd("QTH")))
            .collect();
        assert_eq!(rows, [("K9AAB", "COOK"), ("K9AAB", "DUPG")]);
    }

    /// A new band drops the frequencies read on the old one, and the same band keeps them. A new
    /// mode is what the journal reads back for it, and a new mode class re-fits the RST it was
    /// sent: 59 on phone, the session's 599 on CW and digital.
    #[test]
    fn a_band_or_mode_edit_leaves_the_row_the_journal_reads() {
        let mut l = contest("ilqp", "IL", "KANE");
        l.dial_khz = 14_025;
        l.on_air_hz = 14_025_000;
        assert!(log(&mut l, "K9AAA", &[("RST", "599"), ("QTH", "COOK")], T0));
        let row = |l: &FieldDayLog| {
            let q = &l.qsos()[0];
            (
                q.band.clone(),
                q.mode.clone(),
                q.submode.clone(),
                q.sent("RST").to_string(),
                q.freq_khz,
                q.on_air_hz,
            )
        };
        let rx: &[&[(&str, &str)]] = &[&[("RST", "59"), ("QTH", "COOK")]];
        l.edit_contact("K9AAA", T0, &edit("K9AAA", rx, "20m", "USB"))
            .expect("edited");
        assert_eq!(
            row(&l),
            (
                "20m".into(),
                "PH".into(),
                "USB".into(),
                "59".into(),
                14_025,
                14_025_000
            )
        );
        l.edit_contact("K9AAA", T0, &edit("K9AAA", rx, "40m", "CW"))
            .expect("edited");
        assert_eq!(
            row(&l),
            ("40m".into(), "CW".into(), String::new(), "599".into(), 0, 0)
        );
        l.edit_contact("K9AAA", T0, &edit("K9AAA", rx, "40m", "FT4"))
            .expect("edited");
        assert_eq!(
            row(&l),
            ("40m".into(), "DIG".into(), "FT4".into(), "599".into(), 0, 0)
        );
        // And the journal reads the row back exactly as the edit left it.
        let mut back = contest("ilqp", "IL", "KANE");
        back.restore_journal(&l.journal_adif(), 0, T0 + 60);
        assert_eq!(back.qsos(), l.qsos());
    }

    /// ⭐ **The journal reads an edited log back as it was left** — every row, its mark, its
    /// number and its serial — so a restart keeps the edit and the numbers run on as before.
    #[test]
    fn an_edited_log_reads_back_from_its_journal_as_it_was_left() {
        let mut l = ss();
        l.dial_khz = 14_025;
        l.on_air_hz = 14_025_000;
        assert!(log(&mut l, "K1ABC", &ss_rx("5", "K1ABC", "EMA"), T0));
        assert!(log(&mut l, "K1ABD", &ss_rx("6", "K1ABD", "EMA"), T0 + 60));
        assert!(log(&mut l, "W9AA", &ss_rx("7", "W9AA", "WI"), T0 + 120));
        l.edit_contact(
            "K1ABD",
            T0 + 60,
            &edit("K1ABC", &[&ss_rx("6", "K1ABC", "EMA")], "40m", ""),
        )
        .expect("edited");
        assert_eq!(
            dupes(&l),
            [false, true, false],
            "worked once, whatever the band"
        );
        let mut back = ss();
        back.restore_journal(&l.journal_adif(), 0, T0 + 180);
        assert_eq!(back.qsos(), l.qsos(), "every row as the edit left it");
        assert_eq!(
            (back.max_seq(), back.session.next_serial),
            (l.max_seq(), l.session.next_serial),
            "and the numbers run on from the same place"
        );
    }

    /// What an edit refuses — a contact that is not there (or has no time to name it by), no
    /// call, a band no file can carry, another number of rows, a satellite contact's band — and
    /// that a refusal changes nothing.
    #[test]
    fn an_edit_that_cannot_apply_is_refused_and_changes_nothing() {
        let mut l = fd();
        assert!(log(&mut l, "K2DEF", IL, T0));
        assert!(
            l.log_at("W1AW", "1D", "CT", 0, 0),
            "a legacy row, with no time"
        );
        let leg = SatLeg {
            band: "70cm".into(),
            down_hz: 435_000_000,
            bird: "SO-50".into(),
            single_channel_fm: true,
            name: Some("SO-50".into()),
        };
        assert!(l.log_satellite_at("N0SAT", &pairs(IL), "PH", "FM", &leg, T0 + 60));
        let was = l.qsos().to_vec();
        for (call, when, e, refusal) in [
            (
                "K2DEF",
                T0 + 1,
                edit("K2DEF", &[IL], "20m", ""),
                EditRefusal::Changed,
            ),
            (
                "K2DEX",
                T0,
                edit("K2DEF", &[IL], "20m", ""),
                EditRefusal::Changed,
            ),
            (
                "W1AW",
                0,
                edit("W1AW", &[IL], "20m", ""),
                EditRefusal::Changed,
            ),
            (
                "K2DEF",
                T0,
                edit("  ", &[IL], "20m", ""),
                EditRefusal::Incomplete,
            ),
            (
                "K2DEF",
                T0,
                edit("K2DEF", &[IL], "21m", ""),
                EditRefusal::Incomplete,
            ),
            (
                "K2DEF",
                T0,
                edit("K2DEF", &[IL], "", ""),
                EditRefusal::Incomplete,
            ),
            (
                "K2DEF",
                T0,
                edit("K2DEF", &[IL, IL], "20m", ""),
                EditRefusal::Rows,
            ),
            (
                "N0SAT",
                T0 + 60,
                edit("N0SAT", &[IL], "2m", ""),
                EditRefusal::Satellite,
            ),
        ] {
            assert_eq!(
                l.edit_contact(call, when, &e),
                Err(refusal),
                "{call} {when} {e:?}"
            );
            assert_eq!(l.qsos(), &was[..], "a refusal changes nothing: {e:?}");
        }
        // CONTROL: the satellite contact's own band, and the same edits on the contact named
        // rightly, apply.
        assert!(l
            .edit_contact("N0SAT", T0 + 60, &edit("N0SAU", &[IL], "70CM", ""))
            .is_ok());
        assert!(l
            .edit_contact("K2DEF", T0, &edit("K2DEG", &[IL], "20M", ""))
            .is_ok());
        assert_eq!(calls(&l), ["K2DEG", "W1AW", "N0SAU"]);
    }
}

#[cfg(test)]
mod operator_stats_tests {
    use super::*;
    use crate::contest::{ContestSession, StationData};

    /// ARRL Sweepstakes CW: two points a contact, the section the multiplier, a station
    /// worked once whatever the band.
    fn ss() -> FieldDayLog {
        let rs = crate::fd_rules::ruleset_by_id("arrlss_cw", crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let data = StationData {
            mycall: "W9XYZ".into(),
            fd_section: "WI".into(),
            contest_check: "99".into(),
            contest_category_power: "LOW".into(),
            contest_category_assisted: "NON-ASSISTED".into(),
            ..Default::default()
        };
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        FieldDayLog::new("W9XYZ", session, "20m")
    }

    /// The top of a UTC clock hour.
    const H: u64 = 1_759_996_800;

    fn log(l: &mut FieldDayLog, op: &str, band: &str, call: &str, sec: &str, when: u64) -> bool {
        l.operator = op.into();
        l.band = band.into();
        let fields: Vec<(String, String)> = [
            ("NR", "1"),
            ("PREC", "A"),
            ("CALL", call),
            ("CK", "99"),
            ("SEC", sec),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        l.session.working(call, when);
        let logged = l.log_fields_at(call, &fields, "CW", "", 0, when);
        l.session.logged(call);
        logged
    }

    /// Three operators and one unstamped contact, with one dupe.
    fn club_log() -> FieldDayLog {
        let mut l = ss();
        log(&mut l, "K9AAA", "20m", "K1A", "CT", H + 60);
        log(&mut l, "K9AAA", "20m", "K2B", "NNJ", H + 120);
        log(&mut l, "K9AAA", "20m", "K3C", "EPA", H + 180);
        log(&mut l, "K9AAA", "40m", "K4D", "VA", H + 3660);
        log(&mut l, "N9BBB", "20m", "W5E", "STX", H + 3900);
        log(&mut l, "N9BBB", "20m", "W6F", "CT", H + 4000);
        // K1A again: a Sweepstakes dupe on any band, worth nothing to anyone.
        log(&mut l, "N9BBB", "40m", "K1A", "CT", H + 4100);
        log(&mut l, "W9CCC", "40m", "N7G", "OR", H + 7300);
        log(&mut l, "", "20m", "N8H", "OH", H + 7400);
        l
    }

    const NOW: u64 = H + 7500;

    fn cells(v: &[(&str, &str, u32)]) -> Vec<(String, String, u32)> {
        v.iter()
            .map(|(b, m, n)| (b.to_string(), m.to_string(), *n))
            .collect()
    }

    /// ⭐ Every figure for every operator, by value.
    #[test]
    fn three_operators_get_their_own_figures() {
        let l = club_log();
        let got = l.operator_stats(NOW);
        let want = vec![
            OperatorStats {
                operator: "K9AAA".into(),
                qsos: 4,
                points: 8,
                new_mults: 4,
                last_10_min: 0,
                last_60_min: 0,
                best_hour: (H, 3),
                band_mode: cells(&[("20m", "CW", 3), ("40m", "CW", 1)]),
            },
            OperatorStats {
                operator: "N9BBB".into(),
                qsos: 2,
                points: 4,
                // CT was K9AAA's first.
                new_mults: 1,
                last_10_min: 0,
                // H + 3900 is exactly an hour before NOW, so outside the hour; H + 4000 is in.
                last_60_min: 1,
                best_hour: (H + 3600, 2),
                band_mode: cells(&[("20m", "CW", 2)]),
            },
            // The contact that names nobody is a row of its own, under "".
            OperatorStats {
                operator: "".into(),
                qsos: 1,
                points: 2,
                new_mults: 1,
                last_10_min: 1,
                last_60_min: 1,
                best_hour: (H + 7200, 1),
                band_mode: cells(&[("20m", "CW", 1)]),
            },
            OperatorStats {
                operator: "W9CCC".into(),
                qsos: 1,
                points: 2,
                new_mults: 1,
                last_10_min: 1,
                last_60_min: 1,
                best_hour: (H + 7200, 1),
                band_mode: cells(&[("40m", "CW", 1)]),
            },
        ];
        assert_eq!(got, want);
    }

    /// The operators' figures add up to the log's own: contacts, QSO points and multipliers.
    #[test]
    fn the_operators_sum_to_the_score() {
        let l = club_log();
        let got = l.operator_stats(NOW);
        let (qso_pts, _, mults, _) = l.ruleset().scoring.score(l.score_rows(), 1);
        assert_eq!(
            got.iter().map(|s| s.qsos as usize).sum::<usize>(),
            l.qso_count()
        );
        assert_eq!(l.qso_count(), 8);
        assert_eq!(got.iter().map(|s| s.points).sum::<u32>(), qso_pts);
        assert_eq!(Some(got.iter().map(|s| s.new_mults).sum::<u32>()), mults);
        assert_eq!(mults, Some(7));
    }

    /// A removed contact leaves its operator's figures with it.
    #[test]
    fn a_removed_contact_leaves_the_figures() {
        let mut l = club_log();
        let seq = l.qsos().last().expect("rows").seq;
        assert!(l.remove_row(seq, NOW).is_some());
        let got = l.operator_stats(NOW);
        assert_eq!(
            got.iter().map(|s| s.operator.as_str()).collect::<Vec<_>>(),
            ["K9AAA", "N9BBB", "W9CCC"]
        );
    }

    #[test]
    fn an_empty_log_has_no_operators() {
        assert!(ss().operator_stats(NOW).is_empty());
    }
}
