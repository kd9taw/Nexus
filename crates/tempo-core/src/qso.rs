//! Auto-sequenced FT1 QSO state machine (one [`Station`] per operator), plus a
//! headless loopback driver that runs a full QSO between two stations over the
//! [`crate::channel::VirtualAir`] on alternating slots.
//!
//! Standard exchange (initiator calls CQ, responder answers):
//! ```text
//!   slot 0  A: CQ W9XYZ EN37
//!   slot 1  B: W9XYZ K2DEF FN31
//!   slot 2  A: K2DEF W9XYZ -10
//!   slot 3  B: W9XYZ K2DEF R-12
//!   slot 4  A: K2DEF W9XYZ RR73
//!   slot 5  B: K2DEF W9XYZ 73
//! ```
//! Each station retransmits its current message on its TX slots until it hears
//! the expected reply (the alternating-slot ARQ behavior).

use crate::message::Msg;
use modes::Decode;

/// IR-HARQ redundancy versions per exchange step before the cycle wraps (0,1,2).
const RV_CYCLE: u32 = 3;
/// Max transmissions of one step before the sequencer stops escalating and the
/// step is considered failed (2 full RV cycles). The app may then time out the
/// QSO or return to listening.
pub const MAX_TX_PER_STEP: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Monitoring; will answer the first CQ heard.
    Listening,
    /// Calling CQ; awaiting a grid reply addressed to me.
    CallingCq,
    /// (Responder) sent my grid; awaiting a report addressed to me.
    AwaitReport,
    /// (Initiator) sent a report; awaiting a rogered report.
    AwaitRoger,
    /// (Responder) sent a rogered report; awaiting RR73/RRR.
    AwaitRr73,
    /// (Initiator) sent RR73; awaiting the final 73.
    Confirming,
    /// QSO complete.
    Done,
}

impl State {
    /// Map to WSJT-X `nQSOProgress` (0..5) — the a-priori (AP) pass-schedule
    /// index the golden FT8/FT4 decoder (`ft8b`/`ft4_decode`) keys off via
    /// `naptypes`/`nappasses`. WSJT-X's index is the Tx stage you *last sent*
    /// (its enum `CALLING, REPLYING, REPORT, ROGER_REPORT, ROGERS, SIGNOFF`),
    /// which the decoder uses to predict the partner's *incoming* message and
    /// freeze the known fields (MyCall/DxCall/RRR/73/RR73) in the AP mask.
    ///
    /// Our 7 sequencer states bijection cleanly onto WSJT-X's 6 levels — the
    /// two pre-QSO states (`Listening`/`CallingCq`) both sit at CALLING(0):
    ///
    /// | `State`       | sent (Tx) | nQSOProgress |
    /// |---------------|-----------|--------------|
    /// | Listening     | —         | 0 CALLING    |
    /// | CallingCq     | CQ (Tx6)  | 0 CALLING    |
    /// | AwaitReport   | grid (T1) | 1 REPLYING   |
    /// | AwaitRoger    | rpt (T2)  | 2 REPORT     |
    /// | AwaitRr73     | R+rpt(T3) | 3 ROGER_RPT  |
    /// | Confirming    | RR73 (T4) | 4 ROGERS     |
    /// | Done          | 73 (T5)   | 5 SIGNOFF    |
    pub fn nqso_progress(self) -> i32 {
        match self {
            State::Listening | State::CallingCq => 0,
            State::AwaitReport => 1,
            State::AwaitRoger => 2,
            State::AwaitRr73 => 3,
            State::Confirming => 4,
            State::Done => 5,
        }
    }
}

/// One station's auto-sequencer.
#[derive(Debug, Clone)]
pub struct Station {
    pub mycall: String,
    pub mygrid: String,
    pub dxcall: Option<String>,
    /// The DX station's Maidenhead grid, captured from their CQ/grid message (or
    /// pre-seeded by the operator when starting a directed call). For the log.
    pub dxgrid: Option<String>,
    pub state: State,
    /// Message transmitted on each of my TX slots (None = stay silent / listen).
    pub pending: Option<Msg>,
    /// The signal report I received about my own signal.
    pub rx_report: Option<i32>,
    /// IR-HARQ redundancy version for the next transmission of `pending`: 0 on a
    /// fresh step, escalating 0→1→2→0 each time the step is retransmitted without
    /// the partner advancing (implicit NAK). Reset to 0 when the partner advances
    /// (implicit ACK). Lets the receiver joint-combine retransmissions.
    pub rv_count: u8,
    /// Optional CQ-run call budget: stop calling CQ after this many unanswered
    /// calls. `None` = stock WSJT-X (repeat indefinitely; the Tx watchdog is the
    /// backstop).
    pub cq_call_cap: Option<u32>,
    /// Optional DIRECTED-CALL budget: stop after this many unanswered transmissions of
    /// an in-QSO step (calling a specific station / awaiting the partner to advance the
    /// exchange). `None` = stock WSJT-X (repeat until answered; the Tx watchdog is the
    /// only backstop). Distinct from `cq_call_cap` (which only governs CallingCq); the
    /// engine defaults this to Some(8) so a station that goes silent stops being called.
    pub call_cap: Option<u32>,
    /// Transmissions of the current step so far (resets when the step advances).
    pub tx_count: u32,
    /// Operator preference: roger the final report with `RRR` (acknowledge only,
    /// partner still owes a 73) instead of the combined `RR73`. Default `false`
    /// (RR73 — modern FT8 practice). Mirrors WSJT-X's "Settings ▸ behaviour".
    pub confirm_with_rrr: bool,
    /// Hound (FT8 DXpedition) finish rule: complete on the Fox's RR73 WITHOUT
    /// transmitting a parting 73 (the Fox segment must stay clean). Set by the
    /// engine when the operator's special-op mode is Hound.
    pub quiet_finish: bool,
    /// True once the PARTNER has advanced this exchange at least once on the air —
    /// i.e. [`Self::observe`] moved the state, not [`Self::start`]'s resume table.
    /// Distinguishes a QSO that really ran from a `Done` SYNTHESIZED out of a single
    /// decoded RR73/73 the operator double-clicked (see [`Self::report_impossible_exchange`]).
    pub advanced_on_air: bool,
    /// True once WE have put an over on the air in this QSO — [`Self::after_tx`], the
    /// same event that feeds `tx_count`, but cumulative instead of per-step (`tx_count`
    /// is reset by every advance, so it cannot answer "did we ever key?").
    /// See [`Self::report_impossible_exchange`].
    pub keyed_on_air: bool,
    /// Human-readable event log.
    pub transcript: Vec<String>,
}

/// The operator's grid AS IT GOES ON AIR: the 4-character field+square, upper-cased.
///
/// ⚠️ A STANDARD FT8/FT4 MESSAGE CARRIES FOUR CHARACTERS AND NO MORE (#42). Hand the packer a
/// six-character locator and it cannot build the standard grid message at all — it falls out to
/// free text, which the station you are working cannot auto-sequence against, so the QSO stalls
/// on their side for reasons invisible on yours.
///
/// Baselined against real WSJT-X rather than reasoned about: `MainWindow::genStdMsgs` opens with
/// `auto const& my_grid = m_config.my_grid ().left (4);` and every standard message it builds uses
/// that, as does `genCQMsg` (`grid.left (4)`). So stock
/// truncates ONCE at the message boundary and never transmits six. This is that boundary here:
/// `Station::mygrid` is message content and nothing else reads it.
///
/// The operator's SETTINGS grid is untouched, and so is the logbook — a six-character locator is
/// correct there and is what awards want. Only what leaves the antenna is cut.
fn air_grid(mygrid: &str) -> String {
    mygrid
        .trim()
        .chars()
        .take(4)
        .collect::<String>()
        .to_uppercase()
}

impl Station {
    /// A station that calls CQ to start a QSO.
    pub fn calling_cq(mycall: &str, mygrid: &str) -> Self {
        let mygrid = &air_grid(mygrid);
        Self {
            mycall: mycall.into(),
            mygrid: mygrid.into(),
            dxcall: None,
            dxgrid: None,
            state: State::CallingCq,
            pending: Some(Msg::Cq {
                de: mycall.into(),
                grid: mygrid.into(),
                dir: String::new(),
            }),
            rx_report: None,
            rv_count: 0,
            tx_count: 0,
            cq_call_cap: None,
            call_cap: None,
            confirm_with_rrr: false,
            quiet_finish: false,
            advanced_on_air: false,
            keyed_on_air: false,
            transcript: Vec::new(),
        }
    }

    /// A station that listens and answers the first CQ it hears.
    pub fn monitoring(mycall: &str, mygrid: &str) -> Self {
        let mygrid = &air_grid(mygrid);
        Self {
            mycall: mycall.into(),
            mygrid: mygrid.into(),
            dxcall: None,
            dxgrid: None,
            state: State::Listening,
            pending: None,
            rx_report: None,
            rv_count: 0,
            tx_count: 0,
            cq_call_cap: None,
            call_cap: None,
            confirm_with_rrr: false,
            quiet_finish: false,
            advanced_on_air: false,
            keyed_on_air: false,
            transcript: Vec::new(),
        }
    }

    /// A station that initiates a QSO with a **specific** station — e.g. the
    /// operator clicked a heard station to work them (WSJT-X "double-click to
    /// call"). It sends its grid to `dxcall` and then runs the responder side of
    /// the exchange, exactly as if it had just heard that station's CQ.
    ///
    /// This is [`Station::start`] with no message context — always starts at the
    /// grid (Tx1). Prefer [`start`] when you know the message being answered.
    ///
    /// [`start`]: Station::start
    pub fn answering(mycall: &str, mygrid: &str, dxcall: &str) -> Self {
        Self::start(mycall, mygrid, dxcall, None, false, false)
    }

    /// Begin a directed QSO with `dxcall`, jumping straight to the Tx state the
    /// message we're answering implies — WSJT-X's double-click semantics (its
    /// `processMessage`). `context` is the decoded message we are responding to
    /// (the line the operator double-clicked, or the latest message from `dxcall`
    /// addressed to us) paired with the SNR we decoded it at — that SNR becomes
    /// the report we send back.
    ///
    /// The next message you send is fixed by what the DX last sent **to you**:
    ///
    /// | DX sent (to me)        | I send       | start state  |
    /// |------------------------|--------------|--------------|
    /// | CQ / call / `None`     | my grid (T1) | AwaitReport  |
    /// | my-grid reply (Grid)   | report  (T2) | AwaitRoger   |
    /// | report (Report)        | R+rpt   (T3) | AwaitRr73    |
    /// | R+report (RReport)     | RR73/RRR(T4) | Confirming   |
    /// | RRR / RR73             | 73      (T5) | Done         |
    /// | 73 (Bye73)             | — (log)      | Done         |
    ///
    /// This is the fix for "clicking a station that already answered restarts at
    /// the grid message": a context addressed to me advances the start state, so
    /// answering a station that already sent you a report goes straight to the
    /// R-report — never back to the grid. A CQ / not-addressed-to-me / `None`
    /// context starts at the grid, exactly like working a fresh CQ.
    pub fn start(
        mycall: &str,
        mygrid: &str,
        dxcall: &str,
        context: Option<(&Msg, i32)>,
        prefer_rrr: bool,
        skip_tx1: bool,
    ) -> Self {
        let mycall_s: String = mycall.into();
        let mygrid = &air_grid(mygrid);
        let mut dxgrid: Option<String> = None;
        let mut rx_report: Option<i32> = None;

        // Default: start the exchange — send our grid to dxcall (Tx1).
        let grid_start = || {
            (
                State::AwaitReport,
                Some(Msg::Grid {
                    to: dxcall.into(),
                    de: mycall_s.clone(),
                    grid: mygrid.into(),
                }),
                format!("calling {dxcall} with grid"),
            )
        };

        // Skip Tx1 (WSJT-X parity): when WE initiate a call and already have the DX's
        // SNR (we're answering their CQ), open with the report (Tx2) instead of our grid
        // (Tx1), saving a cycle. No SNR context (a manual call with nothing decoded)
        // falls back to the grid.
        //
        // WHAT `elide_tx1_not_allowed` ACTUALLY MEANS (`mainwindow.cpp:5834`) — read
        // backwards here for a long time. The name parses as "eliding Tx1 is NOT
        // allowed", and its ONLY use upstream is
        // `ui->tx1->setEnabled(elide_tx1_not_allowed() || …)`: returning true FORCES Tx1
        // to stay available. It is not licence to substitute a degraded message — Tx1
        // itself is untouched, and the old claim here ("the report message can't pack a
        // compound/nonstandard call") is measurably false: `W9XYZ F4CYH/P +03` packs and
        // round-trips exactly. What the restriction protects is the hash table — a
        // nonstandard station's partner must copy the full call at least once before the
        // hashed forms mean anything, and Tx1 is where that happens.
        //
        // Upstream's predicate is `Radio::is_77bit_nonstandard_callsign`, which is
        // COARSER than `stdCall` and true for `/P` as well, so a portable station still
        // opens with the grid — even though (unlike before) its report would pack fine.
        let fresh_open = || match context {
            Some((_, rpt)) if skip_tx1 && !crate::message::is_77bit_nonstandard_call(&mycall_s) => {
                (
                    State::AwaitRoger,
                    Some(Msg::Report {
                        to: dxcall.into(),
                        de: mycall_s.clone(),
                        snr: rpt,
                    }),
                    format!("calling {dxcall} with report {rpt} (skip Tx1)"),
                )
            }
            _ => grid_start(),
        };

        // Only a message addressed to *us* advances the start state; a CQ or a
        // message to someone else means we're initiating, so we start at the grid.
        // Base-call comparison so portable/compound calls (KD9TAW/P) still match.
        let to_me = context
            .and_then(|(m, _)| m.addressee())
            .map(|to| crate::message::same_call(to, &mycall_s))
            .unwrap_or(false);

        let (state, pending, log_line) = match context {
            // `rpt` (the SNR we decoded the DX at) is the report we send them.
            Some((msg, rpt)) if to_me => match msg {
                // DX answered our CQ with their grid → send them a report.
                Msg::Grid { de, grid, .. } => {
                    if !grid.is_empty() {
                        dxgrid = Some(grid.clone()); // i3=4 calls carry no grid → keep None
                    }
                    (
                        State::AwaitRoger,
                        Some(Msg::Report {
                            to: de.clone(),
                            de: mycall_s.clone(),
                            snr: rpt,
                        }),
                        format!("{de} answered with grid → sending report {rpt}"),
                    )
                }
                // DX sent us a bare report → roger it with R + our report.
                Msg::Report { de, snr, .. } => {
                    rx_report = Some(*snr);
                    (
                        State::AwaitRr73,
                        Some(Msg::RReport {
                            to: de.clone(),
                            de: mycall_s.clone(),
                            snr: rpt,
                        }),
                        format!(
                            "got report {snr} → sending R{}",
                            crate::message::fmt_report(rpt)
                        ),
                    )
                }
                // DX sent R + report → send the roger (RR73, or RRR by preference).
                Msg::RReport { de, snr, .. } => {
                    rx_report = Some(*snr);
                    let roger = if prefer_rrr {
                        Msg::Rrr {
                            to: de.clone(),
                            de: mycall_s.clone(),
                        }
                    } else {
                        Msg::Rr73 {
                            to: de.clone(),
                            de: mycall_s.clone(),
                        }
                    };
                    (
                        State::Confirming,
                        Some(roger),
                        format!(
                            "got R-report → sending {}",
                            if prefer_rrr { "RRR" } else { "RR73" }
                        ),
                    )
                }
                // DX already rogered (RRR/RR73) → send the final 73.
                Msg::Rrr { de, .. } | Msg::Rr73 { de, .. } => (
                    State::Done,
                    Some(Msg::Bye73 {
                        to: de.clone(),
                        de: mycall_s.clone(),
                    }),
                    "got RR73 → sending 73, QSO complete".into(),
                ),
                // DX already signed 73 → nothing to send; ready to log.
                Msg::Bye73 { .. } => (State::Done, None, "got 73 → QSO complete".into()),
                _ => grid_start(),
            },
            // We're initiating (a CQ, a call to someone else, or no context) — normally
            // Tx1 (grid), or Tx2 (report) when Skip Tx1 is on and we heard their SNR.
            _ => fresh_open(),
        };

        let mut station = Self {
            mycall: mycall_s,
            mygrid: mygrid.into(),
            dxcall: Some(dxcall.into()),
            dxgrid,
            state,
            pending,
            rx_report,
            rv_count: 0,
            tx_count: 0,
            cq_call_cap: None,
            call_cap: None,
            confirm_with_rrr: prefer_rrr,
            quiet_finish: false,
            // `start()` resumes from a message we ALREADY had; the partner has not
            // answered US yet, and nothing has left our antenna for it. Only `observe`
            // sets the first; only `after_tx` sets the second.
            advanced_on_air: false,
            keyed_on_air: false,
            transcript: vec![log_line],
        };
        // The resume table above names the Tx we queue; only now, with both calls in
        // hand, can we tell what that Tx will look like ON THE AIR. A roger the packer
        // renders as a bare `RRR` closes the QSO for the partner, so it puts us in
        // `Confirming`, not `AwaitRr73` — see `roger_state`.
        if station.state == State::AwaitRr73 {
            station.state = station.roger_state();
        }
        station
    }

    pub fn done(&self) -> bool {
        self.state == State::Done && self.pending.is_none()
    }

    /// True when this PAIR of calls cannot ride the plain Type 1/2 forms, so our
    /// outgoing messages must use the hashed i3=4 forms instead — one call in full, the
    /// other wrapped in `<...>`, both recovered verbatim by the receiver.
    ///
    /// Two independent packer facts, and the second is a property of the pair alone:
    ///
    /// 1. **Either call is 77-bit NONSTANDARD.** `pack77_1` would strip the affix off a
    ///    bare nonstandard call silently and name a different station (`CQ PJ4/K1ABC
    ///    FK52` goes out as `CQ K1ABC FK52`). **`/P` and `/R` are NOT nonstandard** —
    ///    they ride Type 1/2 in full, grid and numeric report intact, and using the
    ///    coarse "has a slash" test here is what made a portable station's locator and
    ///    reports disappear.
    /// 2. **The two suffixes conflict** ([`crate::message::suffix_conflict`]). One `i3`
    ///    names what BOTH suffix bits mean, so `/P` beside `/R` is unrepresentable: the
    ///    `/R` station's call comes back off the air as `/P`. Both calls are perfectly
    ///    standard; the PAIR is what the packer cannot express.
    ///
    /// Drives [`Self::hashed_form`] — i.e. what we TRANSMIT — and nothing else.
    fn needs_hashed_form(&self) -> bool {
        let dx = self.dxcall.as_deref();
        !crate::message::is_std_call(&self.mycall)
            || dx.is_some_and(|dx| !crate::message::is_std_call(dx))
            || dx.is_some_and(|dx| crate::message::suffix_conflict(&self.mycall, dx))
    }

    /// True when either call carries a slash at all. **Receive-side tolerance only**:
    /// a partner who chose the hashed forms — a station running a Nexus from before
    /// this fix, say — answers a `/P` QSO grid-less, and the [`Self::observe`] arms
    /// keyed on this must still advance the exchange. Never decides what we send.
    fn is_compound_qso(&self) -> bool {
        crate::message::is_compound(&self.mycall)
            || self
                .dxcall
                .as_deref()
                .is_some_and(crate::message::is_compound)
    }

    /// True when this QSO's overs may arrive — or must go out — in the DEGRADED i3=4
    /// vocabulary, whose ENTIRE payload set is blank / `RRR` / `RR73` / `73`
    /// (`pack77_4` spends 2 bits on `nrpt`; there is no grid field and no report
    /// field). Gates the [`Self::observe`] arm that must accept an EARLY roger — an
    /// `RRR`/`RR73` where a plain QSO would still owe us an R-report — as genuine
    /// progress. **Never decides what we send** — [`Self::hashed_form`] does that.
    ///
    /// **Not the gate for the grid-less floor arm.** That one is
    /// [`Self::needs_hashed_form`] alone: this predicate is deliberately the wider of the
    /// two, and admitting a plain-form `/P` pair to an arm that ROGERS made a portable
    /// operator acknowledge a report nobody had sent.
    ///
    /// Two sources, and BOTH have to be here:
    ///
    /// * [`Self::needs_hashed_form`] — the pairs *we* cannot express in Type 1/2, so
    ///   this is the vocabulary we put on the air ourselves. [`Self::is_compound_qso`]
    ///   alone misses them: a 77-bit nonstandard call need carry no slash at all
    ///   (`YW18FIFA`), and the `/P` × `/R` conflict is a property of neither call on
    ///   its own. Guarding the degraded arms on the slash test is why those pairs
    ///   livelocked — every over was legal, and no arm would look at it.
    /// * [`Self::is_compound_qso`] — receive-side tolerance for a partner who degrades
    ///   where we would not: a `/P` station running a Nexus from before `c745a842`
    ///   answers a plain-form QSO grid-less, and we must still advance.
    fn degraded_vocabulary(&self) -> bool {
        self.needs_hashed_form() || self.is_compound_qso()
    }

    /// True when a NUMERIC report can ride this pair's overs in at least one direction.
    ///
    /// A plain Type 1/2 pair always can. Once the pair falls to the hashed forms, only
    /// a plain c28 call standing opposite the hash may still carry one
    /// ([`crate::message::packs_beside_hash`] — `packjt77.f90:1183-1184` refuses Type
    /// 1/2 to any slashed call beside a hashed token), and it is the SENDER of each
    /// over that decides. So `KD9TAW × PJ4/K1ABC` still exchanges one number (mine);
    /// `KD9TAW/P × F4CYH/R`, `KD9TAW/P × PJ4/K1ABC` and `YW18FIFA × PJ4/K1ABC`
    /// exchange none at all, in either direction, for the whole QSO.
    fn report_can_ride(&self) -> bool {
        if !self.needs_hashed_form() {
            return true;
        }
        crate::message::packs_beside_hash(&self.mycall)
            || self
                .dxcall
                .as_deref()
                .is_some_and(crate::message::packs_beside_hash)
    }

    /// True when a real exchange is under way with this station and NO numeric report
    /// can ever be part of it, **because the protocol has no field for one**
    /// ([`Self::report_can_ride`]) — not because nothing has happened.
    ///
    /// Both log paths otherwise require a report in one direction, as a proxy for "a
    /// real exchange happened": `call_station` can synthesize a [`State::Done`] straight
    /// out of a single decoded RR73/73 the operator double-clicked, and such a seed must
    /// never log a phantom contact. That proxy is measurably wrong for the seven
    /// callsign-class pairs whose every over is i3=4 — they run a genuine QSO to 73 and
    /// can never produce the number the gate demands, so the contact was dropped.
    ///
    /// **What this predicate requires, and why it is the honest test.** A contact is two
    /// stations that heard each other, so the evidence has to be one fact from each end:
    ///
    /// * [`Self::keyed_on_air`] — WE transmitted. Without it a station that only ever
    ///   *listened* logged a contact: queue Tx1, never key, decode the DX's `RR73` to
    ///   somebody, and the state machine walks to `Done` on the partner's messages alone.
    ///   This is the same evidence the `Confirming` log gate has always demanded
    ///   (`tx_count >= 1`), made cumulative — `tx_count` is per-step and every advance
    ///   zeroes it, so at `Done` it is 0 for real QSOs too.
    /// * [`Self::advanced_on_air`] — the PARTNER answered us, in [`Self::observe`] and
    ///   not in [`Self::start`]'s resume table, so a synthesized `Done` stays refused.
    ///
    /// …and a number was impossible throughout ([`Self::report_can_ride`]) — because the
    /// protocol has no field for one, not because nothing happened. The log then records
    /// what really happened: an ABSENT RST, never an invented one.
    pub fn report_impossible_exchange(&self) -> bool {
        self.keyed_on_air && self.advanced_on_air && !self.report_can_ride()
    }

    /// Rewrite a message into its modem-faithful hashed form when the pair cannot ride
    /// Type 1/2 ([`Self::needs_hashed_form`]): wrap the ADDRESSEE in `<...>` (the modem
    /// hashes it) while the sender stays in full, so **both callsigns come back off the
    /// air exactly as queued**. A no-op for a pair the plain forms can carry — which
    /// includes every `/P` and `/R` station working a standard or same-suffixed
    /// partner, so their overs are left exactly as the sequencer built them.
    ///
    /// **What the hashed form costs, stated here so it is not rediscovered:** the
    /// sender keeps a grid or a numeric report only while it is a plain c28 call
    /// ([`crate::message::packs_beside_hash`]). A slashed sender opposite a hash is
    /// refused Type 1/2 by `packjt77.f90:1183-1184` and drops to i3=4, whose only
    /// payloads are blank / `RRR` / `RR73` / `73`. So a `/P` operator working a hashed
    /// DX — and either operator of a `/P` × `/R` pair — sends no grid and no number,
    /// and the roger becomes `RRR`. That is deliberate: **correct-but-degraded beats
    /// wrong-but-rich.** A missing report costs a retry; a wrong callsign costs the
    /// other operator a confirmation they will never get and cannot diagnose.
    fn hashed_form(&self, msg: Msg) -> Msg {
        if !self.needs_hashed_form() {
            return msg;
        }
        let brk = |to: &str| {
            format!(
                "<{}>",
                to.trim().trim_start_matches('<').trim_end_matches('>')
            )
        };
        match msg {
            // A CQ has no addressee to hash, and a hashed CQ is unusable anyway:
            // `CQ <F4CYH/P> JN18` packs but does NOT unpack, so it must never be
            // generated. A standard caller therefore keeps grid and directed token; a
            // nonstandard one sends its call in full and loses both, because i3=4 has a
            // slot for neither (a kept `dir` would fall through to TRUNCATED free text).
            Msg::Cq { de, grid, dir } if crate::message::is_std_call(&de) => {
                Msg::Cq { de, grid, dir }
            }
            Msg::Cq { de, .. } => Msg::Cq {
                de,
                grid: String::new(),
                dir: String::new(),
            },
            // The SENDER decides what the payload can be, and the test is
            // `packs_beside_hash`, NOT `is_std_call`: a plain c28 sender keeps its grid
            // opposite the hash — `<PJ4/K1ABC> KD9TAW EN52` is a hashed-first Type 1 and
            // round-trips verbatim, exactly what upstream builds (`msgtype(t0a +
            // my_grid, ui->tx1)` when only HIS call is nonstandard) — while a SLASHED
            // sender is refused Type 1/2 by `packjt77.f90:1183-1184` and drops to i3=4,
            // which has no grid field. Keeping the grid there would not send it; it
            // would only make Tx1 pack to the same bytes as Tx2 and Tx3.
            Msg::Grid { to, de, grid } => {
                let grid = if crate::message::packs_beside_hash(&de) {
                    grid
                } else {
                    String::new()
                };
                Msg::Grid {
                    to: brk(&to),
                    de,
                    grid,
                }
            }
            // Likewise a numeric report: it survives only while the SENDER is the plain
            // c28 call carrying it opposite the hash. When it is not — a nonstandard
            // call, or a `/P`//`R` call, which the packer refuses beside a hash — i3=4
            // has no report field at all (only RRR/RR73/73), so ROGER instead. That
            // keeps Tx3 distinguishable from Tx1/Tx2, which is what lets the partner's
            // sequencer advance; three overs with identical bytes strand it. The QSO
            // still completes, and the numeric exchange rides the other side's overs
            // whenever that side can carry it.
            Msg::RReport { to, de, .. } if !crate::message::packs_beside_hash(&de) => {
                Msg::Rrr { to: brk(&to), de }
            }
            Msg::Report { to, de, .. } if !crate::message::packs_beside_hash(&de) => Msg::Grid {
                to: brk(&to),
                de,
                grid: String::new(),
            },
            Msg::Report { to, de, snr } => Msg::Report {
                to: brk(&to),
                de,
                snr,
            },
            Msg::RReport { to, de, snr } => Msg::RReport {
                to: brk(&to),
                de,
                snr,
            },
            Msg::Rr73 { to, de } => Msg::Rr73 { to: brk(&to), de },
            Msg::Rrr { to, de } => Msg::Rrr { to: brk(&to), de },
            Msg::Bye73 { to, de } => Msg::Bye73 { to: brk(&to), de },
            other => other,
        }
    }

    /// The state a just-queued ROGER (`pending` = an R-report, Tx3) really leaves us in.
    ///
    /// [`Self::hashed_form`] renders an R-report as a bare `RRR` whenever our own call
    /// cannot carry a number beside a hash ([`crate::message::packs_beside_hash`]) — and
    /// a bare `RRR` is not Tx3 on the air, it is the CLOSING roger: the partner's
    /// `(AwaitRoger, Rrr)` arm takes it as one, logs the contact and signs 73. Staying in
    /// [`State::AwaitRr73`] after sending it parks us in the RESPONDER'S seat, waiting for
    /// an RR73 nobody will ever send — so their closing 73 lost to QSB costs us a contact
    /// the other operator already has, and on a CQ run the run stops with it, while the
    /// plain initiator in [`State::Confirming`] survives exactly the same loss. (Measured:
    /// in every pair whose roger degrades, the CQ RUNNER is the side left holding it.)
    ///
    /// `Confirming` says what we actually did. The app logs on it once the roger has
    /// genuinely gone out (`tx_count >= 1`, the same evidence the plain initiator gives)
    /// and resumes the CQ run from it, and `(Confirming, Bye73)` still closes the QSO when
    /// the 73 does arrive.
    fn roger_state(&self) -> State {
        match self.pending.clone().map(|m| self.hashed_form(m)) {
            Some(Msg::Rrr { .. }) => State::Confirming,
            _ => State::AwaitRr73,
        }
    }

    /// The message to transmit on my next TX slot, if any (RV-agnostic).
    pub fn outgoing(&self) -> Option<Msg> {
        self.pending.clone().map(|m| self.hashed_form(m))
    }

    /// True when the current step's transmission budget is exhausted, so the next over
    /// must be withheld. Two independent budgets, each governing its own state(s):
    /// `cq_call_cap` caps a CQ run (CallingCq only; `None` = stock indefinite), and
    /// `call_cap` caps a directed in-QSO step the partner has stopped advancing
    /// (AwaitReport/AwaitRoger/AwaitRr73/Confirming; the engine defaults it to Some(8)
    /// so a station that goes silent stops being called). A normal QSO never trips
    /// `call_cap` because each step advances (resetting tx_count) within a few overs;
    /// only a stuck/unanswered step accumulates to the cap. Listening/Done never send.
    fn tx_capped(&self) -> bool {
        match self.state {
            State::CallingCq => self.cq_call_cap.is_some_and(|cap| self.tx_count >= cap),
            // The establishing steps of a directed QSO — calling a station and waiting for
            // it to advance the exchange. Confirming is excluded: by then the QSO is made
            // and auto-logged, so it is not "calling someone" (matches the engine's own
            // abandon-stalled state set).
            State::AwaitReport | State::AwaitRoger | State::AwaitRr73 => {
                self.call_cap.is_some_and(|cap| self.tx_count >= cap)
            }
            State::Confirming | State::Listening | State::Done => false,
        }
    }

    /// The message **and IR-HARQ redundancy version** to transmit on my next TX slot.
    /// Returns `None` when there is nothing to send OR the step's [`tx_capped`] budget is
    /// spent. ([`MAX_TX_PER_STEP`] is a separate caller/test-side convention; the Tempo
    /// chat tiers apply their own step cap in the engine.)
    pub fn outgoing_rv(&self) -> Option<(Msg, u8)> {
        if self.tx_capped() {
            return None;
        }
        self.pending
            .clone()
            .map(|m| (self.hashed_form(m), self.rv_count))
    }

    /// Begin the CQ run again after a pause, by forgetting the calls already made.
    ///
    /// A CQ run that hits `cq_call_cap` goes quiet. Nexus waits (the engine owns that clock,
    /// `Settings::cq_pause_secs`) and then calls this to start calling again — eight CQs, a
    /// breather, eight more, which is band courtesy rather than holding a frequency until
    /// something answers.
    ///
    /// ⚠️ ONLY IN `CallingCq`, and that restriction is the safety of it. `tx_count` is what
    /// stops a DIRECTED step being re-sent forever at a station that has gone silent; clearing
    /// it in `AwaitReport`/`AwaitRoger`/`AwaitRr73` would hand back a budget the operator's
    /// `directed_max_calls` deliberately spent, and Nexus would call a dead station until the
    /// watchdog noticed. Returns false when the state is anything else, so a caller that gets
    /// its condition wrong is a no-op rather than a transmit bug.
    ///
    /// Nothing else is touched: not the state, not the pending message, not the partner. The
    /// run resumes; it does not restart from a different place.
    pub fn resume_cq_run(&mut self) -> bool {
        if self.state != State::CallingCq {
            return false;
        }
        self.tx_count = 0;
        true
    }

    /// True when the current step has hit its transmission budget without the partner
    /// advancing — i.e. we have an outgoing message but [`outgoing_rv`] is withholding
    /// it. The app may time out the QSO at this point (stop the CQ run, or abandon a
    /// directed call the station never answered).
    pub fn stalled(&self) -> bool {
        self.pending.is_some() && self.tx_capped()
    }

    /// The current outgoing message as on-air text (the "Now sending" readout),
    /// regardless of whether it is currently being withheld by a stall. `None`
    /// when there is nothing queued (listening, or the QSO is complete).
    pub fn pending_text(&self) -> Option<String> {
        self.pending
            .as_ref()
            .map(|m| self.hashed_form(m.clone()).to_text())
    }

    /// Operator "Resend": re-arm the current step. Clears the retransmission
    /// counter (and HARQ escalation) so a stalled step transmits again on the
    /// next TX slot — the partner did not copy and we want another round.
    /// No-op when there is nothing pending.
    pub fn resend(&mut self) {
        if self.pending.is_some() {
            self.tx_count = 0;
            self.rv_count = 0;
            self.log("operator resend → re-arming current message".into());
        }
    }

    /// Operator override: replace the next transmission with `msg` (e.g. an
    /// in-QSO free-text Tx5, or forcing a specific standard message), starting a
    /// fresh HARQ cycle. The auto-sequencer's [`observe`] still advances on the
    /// matching reply, so a forced resend rejoins the normal flow.
    pub fn override_next(&mut self, msg: Msg) {
        self.log(format!("operator override → {}", msg.to_text()));
        self.pending = Some(msg);
        self.tx_count = 0;
        self.rv_count = 0;
    }

    /// Called after I transmit `pending`. Escalates the IR-HARQ redundancy
    /// version for the next retransmission of the SAME step (0→1→2→0), and counts
    /// transmissions of this step. (A partner advance in [`observe`] resets both,
    /// so at good SNR — where every transmission is acknowledged — RV stays 0.)
    /// Also clears `pending` once the QSO is complete so the final 73 goes once.
    pub fn after_tx(&mut self) {
        self.tx_count = self.tx_count.saturating_add(1);
        self.rv_count = (self.tx_count % RV_CYCLE) as u8;
        // The one place a QSO learns that WE keyed. `tx_count` is per-STEP and every
        // advance zeroes it, so it cannot be asked "did this QSO ever transmit?" —
        // this can (see `report_impossible_exchange`).
        self.keyed_on_air = true;
        if self.state == State::Done {
            self.pending = None;
        }
    }

    fn log(&mut self, s: String) {
        self.transcript.push(s);
    }

    /// True when `sender` is the station we're working (or we haven't locked one
    /// yet). Once a QSO is in progress, the auto-sequencer must only advance on
    /// messages FROM the worked DX — a reply from a different station must not
    /// hijack the sequence (WSJT-X checks the sender against DX Call). Compared on
    /// base calls so a portable suffix still matches.
    #[allow(clippy::wrong_self_convention)]
    fn from_dx(&self, sender: &str) -> bool {
        self.dxcall
            .as_deref()
            .is_none_or(|dx| crate::message::same_call(sender, dx))
    }

    /// Process the signals decoded this RX slot and advance the sequence.
    pub fn observe(&mut self, decodes: &[Decode]) {
        let state_before = self.state;
        for d in decodes {
            // Resolve a HASHED sender here, once, rather than in each arm: every reply below
            // is built as `to: de.clone()` from the message it answers, so an unresolved
            // bracket form would be echoed straight back onto the air (see Msg::unhashed).
            let m = Msg::parse(&d.message).unhashed();
            let rpt = d.snr.clamp(-30, 49);
            match (self.state, &m) {
                // NOTE: there is intentionally NO (Listening, Cq) auto-answer arm.
                // "Monitor" is passive RX — it must NEVER key up on its own (an
                // unsolicited transmission is unacceptable, and it's not how WSJT-X
                // works). The operator works a station explicitly by double-clicking
                // a decode, which builds an `answering`/`start(..)` station.
                (State::CallingCq, Msg::Grid { to, de, grid })
                    if crate::message::same_call(to, &self.mycall) =>
                {
                    // `de` is already unhashed: `observe` runs every parsed message through
                    // `Msg::unhashed`, so a `<W9XYZ>` sender arrives here as `W9XYZ` and an
                    // UNRESOLVED `<...>` arrives untouched (it is not a callsign and must not
                    // be laundered into one).
                    self.dxcall = Some(de.clone());
                    if !grid.is_empty() {
                        self.dxgrid = Some(grid.clone()); // i3=4 calls carry no grid
                    }
                    self.pending = Some(Msg::Report {
                        to: de.clone(),
                        de: self.mycall.clone(),
                        snr: rpt,
                    });
                    self.state = State::AwaitRoger;
                    self.log(format!("{de} answered → sending report {rpt}"));
                }
                (State::AwaitReport, Msg::Report { to, de, snr })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    self.rx_report = Some(*snr);
                    self.pending = Some(Msg::RReport {
                        to: de.clone(),
                        de: self.mycall.clone(),
                        snr: rpt,
                    });
                    self.state = self.roger_state();
                    self.log(format!(
                        "got report {snr} → sending R{}",
                        crate::message::fmt_report(rpt)
                    ));
                }
                (State::AwaitRoger, Msg::RReport { to, de, snr })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    self.rx_report = Some(*snr);
                    // RR73 (combined roger+73, modern default) unless the operator
                    // prefers a bare RRR (roger only; partner still owes a 73).
                    self.pending = Some(if self.confirm_with_rrr {
                        Msg::Rrr {
                            to: de.clone(),
                            de: self.mycall.clone(),
                        }
                    } else {
                        Msg::Rr73 {
                            to: de.clone(),
                            de: self.mycall.clone(),
                        }
                    });
                    self.state = State::Confirming;
                    self.log(format!(
                        "got R-report → sending {}",
                        if self.confirm_with_rrr { "RRR" } else { "RR73" }
                    ));
                }
                (State::AwaitRr73, Msg::Rr73 { to, de })
                | (State::AwaitRr73, Msg::Rrr { to, de })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    if self.quiet_finish {
                        // Hound rule (FT8 DXpedition mode): the Fox's RR73 ends
                        // the QSO — log and STOP. A parting 73 would land in the
                        // Fox's own 300–900 Hz segment: pure QRM the mode exists
                        // to avoid, and stock WSJT-X hounds send nothing here.
                        self.pending = None;
                        self.log("got RR73 → QSO complete (hound: no 73)".into());
                    } else {
                        self.pending = Some(Msg::Bye73 {
                            to: de.clone(),
                            de: self.mycall.clone(),
                        });
                        self.log("got RR73 → sending 73, QSO complete".into());
                    }
                    self.state = State::Done;
                }
                // We rogered and are waiting to be signed off. Any of the three closing
                // words does it: a bare `73`, or the partner's own `RR73`/`RRR`. We owe
                // nothing after our roger — a partner who answers it with a roger of their
                // own (which is what a degraded pair's `RRR` is, and what two stations that
                // both think they are closing send in the plain forms) has confirmed the
                // contact, and re-sending ours at them forever is not a courtesy.
                (State::Confirming, Msg::Bye73 { to, de })
                | (State::Confirming, Msg::Rr73 { to, de })
                | (State::Confirming, Msg::Rrr { to, de })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    self.pending = None;
                    self.state = State::Done;
                    self.log("got the closing over → QSO complete".into());
                }
                // THE PARTNER DID NOT COPY OUR ROGER AND IS ASKING AGAIN (issue #59).
                //
                // We are `Confirming`, so we have sent our closing over and cleared `pending`.
                // If the DX now repeats their report — bare, or R-prefixed — they did not hear
                // it, and the only useful thing we can do is send it again. Without these arms
                // the message fell to `_ => {}`: Nexus went silent at exactly the moment the
                // partner was asking it to speak, and (having already logged) went back to
                // calling CQ. From the operator's chair that is the contact being abandoned
                // mid-close, which is what kr4fqg reported.
                //
                // ⚠️ THIS IS WSJT-X's BEHAVIOUR, not an improvement on it. Real upstream source,
                // `widgets/mainwindow.cpp:6374-6378`:
                //
                //     } else if((m_QSOProgress >= REPORT || (m_QSOProgress >= REPLYING && …))
                //               && word_3.startsWith ('R')) {
                //       m_ntx=4;                    // send the roger AGAIN
                //       m_QSOProgress = ROGERS;
                //
                // Note what is NOT here: the three closing words keep their own arms ABOVE this
                // one and still finish the QSO. A partner's `RRR` is their roger, not a request
                // — answering it would be the "re-sending ours at them forever" this file
                // already warns about, so these arms match only a REPORT.
                (State::Confirming, Msg::Report { to, de, snr })
                | (State::Confirming, Msg::RReport { to, de, snr })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    // Their number, in case the repeat carries a corrected one.
                    self.rx_report = Some(*snr);
                    self.pending = Some(if self.confirm_with_rrr {
                        Msg::Rrr {
                            to: de.clone(),
                            de: self.mycall.clone(),
                        }
                    } else {
                        Msg::Rr73 {
                            to: de.clone(),
                            de: self.mycall.clone(),
                        }
                    });
                    self.log("DX repeated their report → re-sending our closing over".into());
                }
                // --- Out-of-order / step-skipping partners (mirror the `start()` resume
                // table so a running QSO can't hang re-sending the same message forever
                // when the DX skips a step — exactly what WSJT-X handles). ---
                // A caller awaiting a report whose DX combines R + report (skips the bare
                // report): capture it and send the roger.
                (State::AwaitReport, Msg::RReport { to, de, snr })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    self.rx_report = Some(*snr);
                    self.pending = Some(if self.confirm_with_rrr {
                        Msg::Rrr {
                            to: de.clone(),
                            de: self.mycall.clone(),
                        }
                    } else {
                        Msg::Rr73 {
                            to: de.clone(),
                            de: self.mycall.clone(),
                        }
                    });
                    self.state = State::Confirming;
                    self.log(format!(
                        "got R-report → sending {}",
                        if self.confirm_with_rrr { "RRR" } else { "RR73" }
                    ));
                }
                // A caller awaiting a report whose DX rogers directly (RR73/RRR, skipping
                // the report entirely): the DX confirmed → send the final 73 and finish.
                (State::AwaitReport, Msg::Rr73 { to, de })
                | (State::AwaitReport, Msg::Rrr { to, de })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    self.pending = Some(Msg::Bye73 {
                        to: de.clone(),
                        de: self.mycall.clone(),
                    });
                    self.state = State::Done;
                    self.log("DX rogered after our grid → sending 73, QSO complete".into());
                }
                // We're rogering (AwaitRr73, expecting RR73) and the DX closes with a bare
                // 73 instead: that's their roger+signoff → QSO complete.
                (State::AwaitRr73, Msg::Bye73 { to, de })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    self.pending = None;
                    self.state = State::Done;
                    self.log("got 73 → QSO complete".into());
                }
                // We're calling CQ and a station answers with a bare report (grid skipped):
                // lock onto them, capture the report, and roger with R + our report.
                (State::CallingCq, Msg::Report { to, de, snr })
                    if crate::message::same_call(to, &self.mycall) =>
                {
                    // `de` is already unhashed: `observe` runs every parsed message through
                    // `Msg::unhashed`, so a `<W9XYZ>` sender arrives here as `W9XYZ` and an
                    // UNRESOLVED `<...>` arrives untouched (it is not a callsign and must not
                    // be laundered into one).
                    self.dxcall = Some(de.clone());
                    self.rx_report = Some(*snr);
                    self.pending = Some(Msg::RReport {
                        to: de.clone(),
                        de: self.mycall.clone(),
                        snr: rpt,
                    });
                    self.state = self.roger_state();
                    self.log(format!(
                        "{de} answered with a report → R{}",
                        crate::message::fmt_report(rpt)
                    ));
                }
                // A caller awaiting a ROGER whose DX sends a bare report instead: we both
                // sent Tx2. The DX read our report as a call — which is exactly what it
                // looks like once the pair falls to i3=4, where Tx1 and Tx2 pack to the
                // same bytes — so their number is the one that got through. Capture it and
                // roger it, the same resume the `start()` table gives a Report context.
                (State::AwaitRoger, Msg::Report { to, de, snr })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    self.rx_report = Some(*snr);
                    self.pending = Some(Msg::RReport {
                        to: de.clone(),
                        de: self.mycall.clone(),
                        snr: rpt,
                    });
                    self.state = self.roger_state();
                    self.log(format!(
                        "got report {snr} while awaiting a roger → sending R{}",
                        crate::message::fmt_report(rpt)
                    ));
                }
                // --- Compound QSO completion: a compound party can't send a numeric
                // report through i3=4, so the report exchange inverts — the STANDARD
                // station reports and the compound station rogers. These arms advance on
                // the grid-less i3=4 forms the partner actually delivers. ---
                // A GRID addressed to me while I'm awaiting a report → I send MY report
                // and await their roger. Upstream answers a grid addressed to it with a
                // report REGARDLESS of state (`processMessage`), and so does this arm: it
                // is the compound DX answering my call grid-less (no report possible) and
                // it is equally two plain stations that each answered the other's CQ —
                // both then sit in `AwaitReport` believing the other owes the report, and
                // with this arm gated to the degraded vocabulary they traded their grids
                // for as long as anyone watched (measured: 20 slots, no advance). Half-
                // duplex slots break the tie by themselves — whoever decodes first
                // reports, and the other's `(AwaitReport, Report)` arm rogers it — so
                // answering here cannot cross with the partner doing the same.
                (State::AwaitReport, Msg::Grid { to, de, grid })
                    if crate::message::same_call(to, &self.mycall) && self.from_dx(de) =>
                {
                    // `de` is already unhashed: `observe` runs every parsed message through
                    // `Msg::unhashed`, so a `<W9XYZ>` sender arrives here as `W9XYZ` and an
                    // UNRESOLVED `<...>` arrives untouched (it is not a callsign and must not
                    // be laundered into one).
                    self.dxcall.get_or_insert_with(|| de.clone());
                    if !grid.is_empty() {
                        self.dxgrid = Some(grid.clone());
                    }
                    self.pending = Some(Msg::Report {
                        to: de.clone(),
                        de: self.mycall.clone(),
                        snr: rpt,
                    });
                    self.state = State::AwaitRoger;
                    self.log(format!("{de} answered my call → sending report {rpt}"));
                }
                // The compound DX rogered my report (RR73/RRR) → QSO complete, send 73.
                (State::AwaitRoger, Msg::Rr73 { to, de })
                | (State::AwaitRoger, Msg::Rrr { to, de })
                    if self.degraded_vocabulary()
                        && crate::message::same_call(to, &self.mycall)
                        && self.from_dx(de) =>
                {
                    self.pending = Some(Msg::Bye73 {
                        to: de.clone(),
                        de: self.mycall.clone(),
                    });
                    self.state = State::Done;
                    self.log("compound DX rogered → sending 73, QSO complete".into());
                }
                // THE FLOOR OF THE DEGRADED VOCABULARY, and the arm whose absence
                // livelocked SEVEN of the sixteen callsign-class pairs — every pair whose
                // overs are all i3=4. i3=4 has ONE blank payload, so a partner's
                // Tx1 ("calling you") and Tx2 ("here is your report") are the same bytes:
                // `<me> them`. Both stations therefore reach AwaitRoger believing they owe
                // nothing but a roger, and — with no arm here — trade that identical blank
                // over slot after slot forever, never advancing, never logging.
                //
                // Tx1 ≡ Tx2 is the protocol's floor, not a fault to be encoded around: the
                // RECEIVER'S OWN STATE is what disambiguates them, exactly as it already
                // does in the working nonstandard-sender quadrants. Awaiting a roger means
                // we have already sent our report; a grid-less over from the DX is then
                // their answer to it, whatever they meant by it, and the exchange moves on
                // to the roger. A number is not merely absent here — the frame HAS no
                // field for one — so the roger is what advances, and `hashed_form` renders
                // it as the `RRR` i3=4 can actually carry.
                //
                // Narrow on purpose: only a GRID-LESS over, and only in a QSO whose
                // vocabulary is degraded. In a plain QSO a repeated grid means the DX never
                // copied our report, and advancing on it would roger a report they never
                // sent — so a Grid carrying a grid, or any pair the plain forms can express,
                // falls through to the retransmit as before.
                //
                // And the gate is [`Self::needs_hashed_form`], NOT `degraded_vocabulary`,
                // which is the wider of the two: `is_compound_qso` is merely "either call
                // has a slash", and a `/P` or `/R` station opposite a standard (or same-
                // suffixed) partner rides Type 1/2 with grid and number intact. Letting
                // that pair in here is the very thing the paragraph above forbids — a
                // portable operator rogering a report nobody sent. The receive-side
                // tolerance `is_compound_qso` was added for (a partner who degrades where
                // we would not) is served one step earlier, by the ungated
                // `(AwaitReport, Grid)` arm, which answers any grid with a report.
                (State::AwaitRoger, Msg::Grid { to, de, grid })
                    if grid.is_empty()
                        && self.needs_hashed_form()
                        && crate::message::same_call(to, &self.mycall)
                        && self.from_dx(de) =>
                {
                    self.pending = Some(Msg::RReport {
                        to: de.clone(),
                        de: self.mycall.clone(),
                        snr: rpt,
                    });
                    self.state = self.roger_state();
                    self.log(
                        "DX answered grid-less (i3=4 cannot carry a report) → rogering".into(),
                    );
                }
                _ => {}
            }
        }
        // Implicit ACK: the partner advanced us to a new step, so the next
        // transmission is a fresh message — restart the RV escalation at 0.
        if self.state != state_before {
            self.rv_count = 0;
            self.tx_count = 0;
            // …and the partner answering us at all is what tells a real QSO from a
            // `start()` seed. See `completed_report_less`.
            self.advanced_on_air = true;
        }
    }
}

/// One transmission heard on the (virtual) air.
#[derive(Debug, Clone)]
pub struct AirLog {
    pub slot: u64,
    pub from: String,
    pub text: String,
}

/// Run a full QSO between two stations over an in-process virtual channel.
///
/// `a` transmits on even slots, `b` on odd. Each transmitted frame is placed in
/// the channel (on-time, at `snr_db`, with AWGN) and decoded by the other
/// station via the full acquisition path. Stops when both stations are done or
/// `max_slots` is reached. Returns the on-air transcript.
pub fn run_loopback_qso(
    a: &mut Station,
    b: &mut Station,
    snr_db: f32,
    max_slots: u64,
) -> Vec<AirLog> {
    use crate::channel::{to_i16, VirtualAir, ON_TIME_OFFSET};
    use crate::tx;

    let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, 0xC0FFEE);
    let mut log = Vec::new();

    for slot in 0..max_slots {
        let (txs, rxs): (&mut Station, &mut Station) = if slot % 2 == 0 {
            (&mut *a, &mut *b)
        } else {
            (&mut *b, &mut *a)
        };

        if let Some(msg) = txs.outgoing() {
            let text = msg.to_text();
            let frame = tx::build(&text, tempo_fast::SAMPLE_RATE, 1500.0);
            let rx_f32 = air.receive(&frame.wave, ON_TIME_OFFSET, snr_db);
            let iwave = to_i16(&rx_f32);
            let decodes: Vec<Decode> = tempo_fast::decode_frame(
                &iwave,
                200,
                2900,
                3,
                rxs.mycall.as_str(),
                txs.mycall.as_str(),
                0,
                (slot as i64).wrapping_mul(4000), // monotonic ms for IR-HARQ keying
            )
            .into_iter()
            .map(Into::into)
            .collect();
            log.push(AirLog {
                slot,
                from: txs.mycall.clone(),
                text,
            });
            rxs.observe(&decodes);
            txs.after_tx();
        }

        if a.done() && b.done() {
            break;
        }
    }
    log
}

#[cfg(test)]
mod nqso_progress_tests {
    use super::*;

    #[test]
    fn maps_states_to_wsjtx_nqso_progress_bijectively() {
        // The two pre-QSO states both sit at CALLING(0); the rest map 1:1 onto
        // WSJT-X's CALLING..SIGNOFF (0..5), which selects the AP pass schedule.
        assert_eq!(State::Listening.nqso_progress(), 0);
        assert_eq!(State::CallingCq.nqso_progress(), 0);
        assert_eq!(State::AwaitReport.nqso_progress(), 1);
        assert_eq!(State::AwaitRoger.nqso_progress(), 2);
        assert_eq!(State::AwaitRr73.nqso_progress(), 3);
        assert_eq!(State::Confirming.nqso_progress(), 4);
        assert_eq!(State::Done.nqso_progress(), 5);
    }

    #[test]
    fn nqso_progress_is_always_in_decoder_range() {
        // naptypes/nappasses in ft8b/ft4_decode are dimensioned (0:5); an
        // out-of-range index is an out-of-bounds read in the Fortran. Guard it.
        for st in [
            State::Listening,
            State::CallingCq,
            State::AwaitReport,
            State::AwaitRoger,
            State::AwaitRr73,
            State::Confirming,
            State::Done,
        ] {
            let p = st.nqso_progress();
            assert!((0..=5).contains(&p), "{st:?} -> {p} out of 0..=5");
        }
    }
}

#[cfg(test)]
mod start_context_tests {
    //! WSJT-X double-click semantics: starting a directed QSO jumps to the Tx
    //! state implied by the message we're answering (its `processMessage`). The
    //! bug this guards: clicking a station that already answered us must NOT reset
    //! to the grid (Tx1) — it must advance to the correct next message.
    use super::*;
    use crate::message::Msg;

    /// ISSUE #42 — "FTx sends 6 element Maidenhead grid. Expected 4 element, got six."
    ///
    /// A standard FT8/FT4 message has room for four characters of locator. Six cannot be packed
    /// as the standard grid message at all, so it degrades to free text — which the station you
    /// are working cannot auto-sequence against. The QSO stalls on their side for a reason that
    /// is invisible on yours.
    ///
    /// Nexus's CQ TEXT builder already truncated (engine.rs, `chars().take(4)`); the QSO exchange
    /// did not, so an operator with a six-character locator in Settings called CQ correctly and
    /// then put six characters on the air in Tx1. Every constructor goes through `air_grid` now.
    ///
    /// Parity, not preference: WSJT-X `MainWindow::genStdMsgs` opens with `my_grid ().left (4)`
    /// and `genCQMsg` uses `grid.left (4)`.
    #[test]
    fn a_six_character_locator_goes_on_air_as_four() {
        let s = Station::start("KD9TAW", "EN52ab", "W1AW", None, false, false);
        assert_eq!(
            s.outgoing().expect("Tx1 is pending").to_text(),
            "W1AW KD9TAW EN52",
            "a standard message carries four characters of grid and no more"
        );
        assert_eq!(s.mygrid, "EN52");

        let cq = Station::calling_cq("KD9TAW", "EN52ab");
        assert_eq!(
            cq.outgoing().expect("CQ is pending").to_text(),
            "CQ KD9TAW EN52"
        );

        assert_eq!(Station::monitoring("KD9TAW", "EN52ab").mygrid, "EN52");
    }

    /// Four characters are untouched, and a lower-case locator comes out matching the CQ path —
    /// the packer is case-insensitive, so this is about the two paths agreeing, not about the air.
    #[test]
    fn a_four_character_locator_is_unchanged_and_case_is_normalised() {
        let s = Station::start("KD9TAW", "EN52", "W1AW", None, false, false);
        assert_eq!(s.outgoing().unwrap().to_text(), "W1AW KD9TAW EN52");
        let lower = Station::start("KD9TAW", "en52ab", "W1AW", None, false, false);
        assert_eq!(lower.outgoing().unwrap().to_text(), "W1AW KD9TAW EN52");
    }

    const ME: &str = "KD9TAW";
    const MY_GRID: &str = "EN61";
    const DX: &str = "W9XYZ";

    fn start(text: &str, snr: i32) -> Station {
        let m = Msg::parse(text);
        Station::start(ME, MY_GRID, DX, Some((&m, snr)), false, false)
    }

    #[test]
    fn clicking_a_cq_starts_at_the_grid() {
        let s = start("CQ W9XYZ FN31", -7);
        assert_eq!(s.state, State::AwaitReport);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW EN61"));
    }

    #[test]
    fn no_context_starts_at_the_grid() {
        let s = Station::start(ME, MY_GRID, DX, None, false, false);
        assert_eq!(s.state, State::AwaitReport);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW EN61"));
    }

    #[test]
    fn dx_answered_my_cq_with_grid_sends_report() {
        // I called CQ; DX replied with their grid addressed to me → I send a report
        // (the SNR I decoded them at), NOT my grid. dxgrid is captured for the log.
        let s = start("KD9TAW W9XYZ FN31", -12);
        assert_eq!(s.state, State::AwaitRoger);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW -12"));
        assert_eq!(s.dxgrid.as_deref(), Some("FN31"));
    }

    #[test]
    fn dx_sent_a_report_sends_r_report() {
        // The user's exact bug: they sent their call, DX came back with a report;
        // clicking must send R+report, not the grid square.
        let s = start("KD9TAW W9XYZ -09", -11);
        assert_eq!(s.state, State::AwaitRr73);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW R-11"));
        assert_eq!(s.rx_report, Some(-9), "captured the report DX gave us");
    }

    #[test]
    fn dx_sent_r_report_sends_rr73() {
        let s = start("KD9TAW W9XYZ R-15", -8);
        assert_eq!(s.state, State::Confirming);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW RR73"));
        assert_eq!(s.rx_report, Some(-15));
    }

    #[test]
    fn dx_sent_r_report_with_rrr_preference_sends_rrr() {
        let m = Msg::parse("KD9TAW W9XYZ R-15");
        let s = Station::start(ME, MY_GRID, DX, Some((&m, -8)), true, false);
        assert_eq!(s.state, State::Confirming);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW RRR"));
    }

    /// ISSUE #59 — A PARTNER WHO RE-SENDS THEIR REPORT MUST GET OUR ROGER AGAIN.
    ///
    /// kr4fqg: "the system misses retransmissions of signal reports on FT, closing the contact
    /// and moving to the next." He proposed moving the LOGGING trigger; that would be the wrong
    /// fix — Nexus already logs where WSJT-X logs, on transmitting the first 73/RR73 and on
    /// receiving one. The actual gap is here: `Confirming` answers only the three closing words,
    /// so a partner who did not copy our roger and re-sends their report falls through to
    /// `_ => {}` and we go silent at exactly the moment they are asking us to speak.
    ///
    /// WSJT-X re-engages, read from real upstream source
    /// (`widgets/mainwindow.cpp:6374-6378`):
    ///
    /// ```text
    /// } else if((m_QSOProgress >= REPORT || (m_QSOProgress >= REPLYING && FT8/FT4/...))
    ///           && word_3.startsWith ('R')) {
    ///   m_ntx=4;                       // Tx4 = R+report — send the roger AGAIN
    ///   m_QSOProgress = ROGERS;
    /// ```
    #[test]
    fn a_repeated_r_report_while_confirming_re_sends_our_roger() {
        let mut s = start("KD9TAW W9XYZ R-15", -8);
        assert_eq!(s.state, State::Confirming);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW RR73"));
        // We transmit our closing over, and the pending message clears.
        s.pending = None;
        // The partner did not copy it and asks again with the same R+report.
        s.observe(&[dec("KD9TAW W9XYZ R-15", -8)]);
        assert_eq!(
            s.pending_text().as_deref(),
            Some("W9XYZ KD9TAW RR73"),
            "they are still asking — WSJT-X re-sends the roger here, and going quiet is what \
             the operator saw as the contact being abandoned"
        );
        assert_eq!(s.state, State::Confirming, "still closing, not finished");
    }

    /// The same for a BARE repeated report (no R prefix) — the partner never got as far as
    /// rogering us, so they are still asking for the step before.
    #[test]
    fn a_repeated_bare_report_while_confirming_re_sends_our_roger() {
        let mut s = start("KD9TAW W9XYZ R-15", -8);
        s.pending = None;
        s.observe(&[dec("KD9TAW W9XYZ -15", -8)]);
        assert_eq!(
            s.pending_text().as_deref(),
            Some("W9XYZ KD9TAW RR73"),
            "a bare repeat is still a partner asking to be rogered"
        );
    }

    /// ⚠️ THE GUARD ON THE FIX. A partner's own CLOSING word must still finish the QSO — the new
    /// arms must not turn a completed contact into an endless exchange of rogers. This is the
    /// regression the existing comment above the Confirming arms warns about in prose
    /// ("re-sending ours at them forever is not a courtesy"), stated as a test.
    #[test]
    fn a_closing_word_still_ends_it_after_the_re_engagement_arms() {
        for closing in ["KD9TAW W9XYZ RR73", "KD9TAW W9XYZ RRR", "KD9TAW W9XYZ 73"] {
            let mut s = start("KD9TAW W9XYZ R-15", -8);
            s.pending = None;
            s.observe(&[dec(closing, -8)]);
            assert_eq!(s.state, State::Done, "{closing} must complete the QSO");
            assert_eq!(s.pending, None, "{closing} must leave nothing to send");
        }
    }

    #[test]
    fn dx_sent_rr73_sends_final_73() {
        let s = start("KD9TAW W9XYZ RR73", -8);
        assert_eq!(s.state, State::Done);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW 73"));
    }

    #[test]
    fn dx_sent_73_completes_with_nothing_to_send() {
        let s = start("KD9TAW W9XYZ 73", -8);
        assert_eq!(s.state, State::Done);
        assert!(s.pending.is_none());
        assert!(s.done());
    }

    #[test]
    fn working_a_nonstandard_dx_completes_with_realistic_hashed_forms() {
        // Click a genuinely NONSTANDARD DXpedition's CQ (a prefix form, which the packer
        // cannot carry in full alongside a payload) and run the QSO with the forms the
        // REAL modem delivers: the DX answers/rogers grid-less and I — the standard
        // station — carry the numbers. Every over hashes the DX rather than sending it
        // bare, which `pack77_1` would silently rewrite to a different station.
        let cq = Msg::parse("CQ PJ4/K1ABC");
        let mut s = Station::start(ME, MY_GRID, "PJ4/K1ABC", Some((&cq, -10)), false, false);
        // Tx1: DX hashed, my call the standard c28 — so MY grid still rides along
        // (hashed-first Type 1, exactly upstream's `msgtype(t0a + my_grid, ui->tx1)`).
        assert_eq!(s.pending_text().as_deref(), Some("<PJ4/K1ABC> KD9TAW EN61"));
        // The nonstandard DX answers grid-less (it cannot send a numeric report).
        s.observe(&[dec("<KD9TAW> PJ4/K1ABC", -7)]);
        assert_eq!(s.state, State::AwaitRoger);
        // I send MY report — it survives because I'm the standard c28 sender.
        assert_eq!(s.pending_text().as_deref(), Some("<PJ4/K1ABC> KD9TAW -07"));
        // The DX rogers → I send the final 73; the QSO completes.
        s.observe(&[dec("<KD9TAW> PJ4/K1ABC RR73", -7)]);
        assert_eq!(s.state, State::Done);
        assert_eq!(s.pending_text().as_deref(), Some("<PJ4/K1ABC> KD9TAW 73"));
        assert_eq!(
            s.dxcall.as_deref(),
            Some("PJ4/K1ABC"),
            "logs the FULL nonstandard call"
        );
    }

    #[test]
    fn nonstandard_me_running_cq_completes_against_a_standard_caller() {
        // I run CQ as a genuinely nonstandard station (PJ4/K1ABC). A standard caller
        // answers + reports me (a standard sender CAN carry a number); I roger (i3=4
        // can't carry MY number) and the QSO completes — never emitting a phantom number
        // the modem would drop. This is the protocol's constraint, and it applies HERE
        // and not to `/P` (see `portable_*` in tests/portable_suffix_air.rs).
        let mut s = Station::calling_cq("PJ4/K1ABC", MY_GRID);
        assert_eq!(
            s.pending_text().as_deref(),
            Some("CQ PJ4/K1ABC"),
            "nonstandard CQ, no grid — and never hashed, a hashed CQ does not unpack"
        );
        // W9XYZ answers with a bare report (me hashed, them the c28 sender → survives).
        s.observe(&[dec("<PJ4/K1ABC> W9XYZ -09", -8)]);
        assert_eq!(s.dxcall.as_deref(), Some("W9XYZ"));
        assert_eq!(
            s.rx_report,
            Some(-9),
            "captured the standard caller's report"
        );
        // My roger degrades to RRR (nonstandard sender → no numeric); QSO advances.
        assert_eq!(s.pending_text().as_deref(), Some("<W9XYZ> PJ4/K1ABC RRR"));
        // The caller closes → I'm done.
        s.observe(&[dec("<PJ4/K1ABC> W9XYZ RR73", -8)]);
        assert_eq!(s.state, State::Done);
    }

    #[test]
    fn portable_mycall_still_matches_a_reply_to_the_base_call() {
        // I operate as KD9TAW/P; the DX reports my base call KD9TAW. The QSO must still
        // resume, not stall at the grid — and the over it resumes with is the PLAIN
        // Type 1 R-report, both calls in the clear. `/P` rides its own suffix bit, so
        // nothing is hashed and nothing is dropped; this used to degrade to
        // "<W9XYZ> KD9TAW/P RRR", losing the number for no protocol reason at all.
        let m = Msg::parse("KD9TAW W9XYZ -09");
        let s = Station::start("KD9TAW/P", MY_GRID, DX, Some((&m, -11)), false, false);
        assert_eq!(s.state, State::AwaitRr73);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW/P R-11"));
    }

    #[test]
    fn message_addressed_to_someone_else_starts_at_grid() {
        // DX is working another station — clicking DX means I initiate, so grid.
        let s = start("N0ABC W9XYZ -05", -8);
        assert_eq!(s.state, State::AwaitReport);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW EN61"));
    }

    #[test]
    fn skip_tx1_opens_with_the_report_for_a_standard_call() {
        // WSJT-X "Skip Tx1": answering a CQ with skip_tx1 on opens at Tx2 (the report),
        // not Tx1 (the grid) — one cycle saved. The DX's SNR (-12) becomes our report.
        let cq = Msg::parse("CQ W9XYZ EN37");
        let s = Station::start(ME, MY_GRID, DX, Some((&cq, -12)), false, true);
        assert_eq!(s.state, State::AwaitRoger);
        assert!(
            matches!(s.pending, Some(Msg::Report { snr: -12, .. })),
            "opened with the report, not the grid: {:?}",
            s.pending
        );
    }

    #[test]
    fn skip_tx1_falls_back_to_grid_for_a_77bit_nonstandard_call() {
        // Upstream's `elide_tx1_not_allowed()` FORCES Tx1 to stay available for a 77-bit
        // nonstandard call — it does not authorise a degraded message (the old comment
        // here claimed "the report message can't pack a compound/nonstandard call", which
        // is measurably false: `W9XYZ KD9TAW/P +03` packs and round-trips exactly). The
        // real reason is the hash table: the partner must copy the full call once first.
        //
        // The gate is `Radio::is_77bit_nonstandard_callsign`, which is COARSER than
        // `stdCall` — it has no `/R`//`P` exemption, so a portable station still opens
        // with the grid, and so does a no-slash nonstandard call that the old
        // "has a slash" test waved straight through.
        let cq = Msg::parse("CQ W9XYZ EN37");
        for mycall in ["KD9TAW/P", "PJ4/K1ABC", "YW18FIFA"] {
            let s = Station::start(mycall, MY_GRID, DX, Some((&cq, -12)), false, true);
            assert_eq!(s.state, State::AwaitReport, "{mycall} must not elide Tx1");
            assert!(
                matches!(s.pending, Some(Msg::Grid { .. })),
                "{mycall} opens with the grid: {:?}",
                s.pending
            );
        }
    }

    #[test]
    fn skip_tx1_off_opens_with_the_grid() {
        // Baseline: without skip_tx1, answering a CQ opens with the grid (Tx1) as before.
        let cq = Msg::parse("CQ W9XYZ EN37");
        let s = Station::start(ME, MY_GRID, DX, Some((&cq, -12)), false, false);
        assert_eq!(s.state, State::AwaitReport);
        assert!(matches!(s.pending, Some(Msg::Grid { .. })));
    }

    #[test]
    fn resumed_qso_then_advances_normally_via_observe() {
        // Resume at "DX sent report" (→ we send R-report), then the partner sends
        // RR73 → we advance to the final 73 through the normal observe() path.
        let mut s = start("KD9TAW W9XYZ -09", -11);
        assert_eq!(s.state, State::AwaitRr73);
        let rr73 = Msg::Rr73 {
            to: ME.into(),
            de: DX.into(),
        };
        s.observe(&[Decode {
            message: rr73.to_text(),
            sync: 1.0,
            snr: 0,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }]);
        assert_eq!(s.state, State::Done);
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW 73"));
    }

    fn dec(text: &str, snr: i32) -> Decode {
        Decode {
            message: text.into(),
            sync: 1.0,
            snr,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    /// FIELD REPORT 2026-08-23 (KD9TAW working RI1FJL, a Franz Josef Land DXpedition running
    /// multi-answering): Nexus transmitted `<RI1FJL> KD9TAW EN52` for two overs — the DX's call
    /// in its i3=4 HASHED form, inside our own outgoing message.
    ///
    /// A station answering several callers at once sends its own call hashed to make room
    /// (WSJT-X's Fox does it explicitly: `fox_tx.f90` formats `CALL RR73; CALL <FOXCALL> rpt`,
    /// and MSHV's multi-answering has the same pressure on message bits). Matching already
    /// copes — `base_call` strips the brackets, so the sequencer correctly recognises the
    /// sender — but the raw token was STORED as `dxcall` and every message built from it then
    /// rendered the brackets onto the air.
    ///
    /// That is wrong on the air and not what WSJT-X sends: the hashed form is a bit-saving
    /// encoding of a call the receiver is expected to have already, not a way to address
    /// somebody. `unhash_call` exists for exactly this and is documented for it; the adoption
    /// sites simply were not using it.
    #[test]
    fn a_hashed_sender_is_never_adopted_as_the_dx_call() {
        // The DX answers our CQ with its call hashed.
        let mut s = Station::calling_cq(ME, MY_GRID);
        s.observe(&[dec("KD9TAW <W9XYZ> EN37", -5)]);
        assert_eq!(
            s.dxcall.as_deref(),
            Some("W9XYZ"),
            "the brackets are an encoding, not part of the callsign"
        );
        assert_eq!(
            s.pending_text().as_deref(),
            Some("W9XYZ KD9TAW -05"),
            "and they must never reach the air"
        );

        // Same via a REPORT rather than a grid — the other unconditional adoption site.
        let mut r = Station::calling_cq(ME, MY_GRID);
        r.observe(&[dec("KD9TAW <W9XYZ> -12", -8)]);
        assert_eq!(r.dxcall.as_deref(), Some("W9XYZ"));
        assert!(
            !r.pending_text().unwrap_or_default().contains('<'),
            "no hashed token in an outgoing message, got {:?}",
            r.pending_text()
        );

        // CONTROL: a plain call is untouched — this must not be mangling ordinary calls.
        let mut p = Station::calling_cq(ME, MY_GRID);
        p.observe(&[dec("KD9TAW W9XYZ EN37", -5)]);
        assert_eq!(p.dxcall.as_deref(), Some("W9XYZ"));
    }

    /// The control that keeps the unhashing honest: an UNRESOLVED hash is not a callsign.
    ///
    /// `<...>` is what a decoder prints when it has not yet heard the full call. Stripping its
    /// brackets would hand the sequencer the literal `...` as a station to work and to log.
    #[test]
    fn an_unresolved_hash_is_not_turned_into_a_callsign() {
        assert_eq!(crate::message::resolve_hashed("<...>"), "<...>");
        assert_eq!(
            crate::message::resolve_hashed("<W9XYZ>"),
            "W9XYZ",
            "control: a real one IS resolved"
        );
        // ⚠️ ISSUE #84's RULE, and the one this change nearly broke: a COMPOUND call is hashed
        // because it does not fit an ordinary frame. Unwrapping it would build a message the
        // protocol cannot carry, so the brackets stay all the way to the air. (The LOG strips
        // them separately, at the record boundary — a different question about the same token.)
        assert_eq!(crate::message::resolve_hashed("<KH8/W1AW>"), "<KH8/W1AW>");
        assert_eq!(crate::message::resolve_hashed("<PJ4/K1ABC>"), "<PJ4/K1ABC>");

        let mut s = Station::calling_cq(ME, MY_GRID);
        s.observe(&[dec("KD9TAW <...> EN37", -5)]);
        assert_ne!(
            s.dxcall.as_deref(),
            Some("..."),
            "an unknown station must never be adopted as the literal ellipsis"
        );
        // It stays visibly unresolved rather than becoming a plausible-looking call — which is
        // the pre-existing behaviour, and the right one: nothing downstream can mistake it.
        assert_eq!(s.dxcall.as_deref(), Some("<...>"));
    }

    #[test]
    fn locked_qso_ignores_a_different_station() {
        // Working W9XYZ (we sent our grid, awaiting their report). A REPORT from a
        // DIFFERENT station addressed to us must NOT advance our sequence — only the
        // station we're working can (WSJT-X sender check). Then the real DX's report
        // does advance it.
        let mut s = Station::answering(ME, MY_GRID, DX); // dxcall = W9XYZ, AwaitReport
        assert_eq!(s.state, State::AwaitReport);
        s.observe(&[dec("KD9TAW N0ABC -05", -5)]); // a different station reports us
        assert_eq!(
            s.state,
            State::AwaitReport,
            "a non-DX reply must not advance"
        );
        s.observe(&[dec("KD9TAW W9XYZ -12", -8)]); // the worked DX reports us
        assert_eq!(
            s.state,
            State::AwaitRr73,
            "the worked DX advances the sequence"
        );
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW R-08"));
    }

    // --- Step-skipping partners: the sequencer must complete, not hang re-sending. ---

    #[test]
    fn dx_rogers_after_our_grid_skipping_the_report() {
        // We answered a CQ (sent our grid, AwaitReport). The DX rogers directly with
        // RR73 (skipping the bare report) → we send 73 and finish, NOT re-send the grid.
        let mut s = Station::answering(ME, MY_GRID, DX);
        assert_eq!(s.state, State::AwaitReport);
        s.observe(&[dec("KD9TAW W9XYZ RR73", -8)]);
        assert_eq!(s.state, State::Done, "an early RR73 completes the QSO");
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW 73"));
    }

    #[test]
    fn dx_sends_combined_r_report_after_our_grid() {
        // AwaitReport, DX combines R + report → capture it and send the roger.
        let mut s = Station::answering(ME, MY_GRID, DX);
        s.observe(&[dec("KD9TAW W9XYZ R-13", -7)]);
        assert_eq!(s.state, State::Confirming);
        assert_eq!(s.rx_report, Some(-13));
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW RR73"));
    }

    #[test]
    fn dx_closes_with_bare_73_instead_of_rr73() {
        // We sent our R-report (AwaitRr73). The DX closes with a plain 73 → QSO complete
        // (instead of re-sending the R-report forever waiting for an RR73).
        let mut s = Station::answering(ME, MY_GRID, DX);
        s.observe(&[dec("KD9TAW W9XYZ -12", -8)]); // their report → AwaitRr73
        assert_eq!(s.state, State::AwaitRr73);
        s.observe(&[dec("KD9TAW W9XYZ 73", -8)]); // a bare 73 instead of RR73
        assert_eq!(s.state, State::Done, "a bare 73 closes the QSO");
    }

    #[test]
    fn cq_answered_with_a_bare_report_locks_and_rogers() {
        // Calling CQ; a station answers with a bare report (grid skipped) → lock onto
        // them, capture the report, and send R + our report.
        let mut s = Station::calling_cq(ME, MY_GRID);
        assert_eq!(s.state, State::CallingCq);
        s.observe(&[dec("KD9TAW W9XYZ -15", -10)]);
        assert_eq!(s.state, State::AwaitRr73);
        assert_eq!(s.dxcall.as_deref(), Some("W9XYZ"));
        assert_eq!(s.rx_report, Some(-15), "captured the report they gave us");
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW R-10"));
    }

    /// EIGHT CQs, A BREATHER, EIGHT MORE — the resume half of the operator's auto-CQ ruling.
    /// `resume_cq_run` is the state part; the engine owns the clock.
    #[test]
    fn a_capped_cq_run_can_be_started_again() {
        let mut s = Station::calling_cq("KD9TAW", "EN52");
        s.cq_call_cap = Some(3);
        for _ in 0..3 {
            assert!(s.outgoing_rv().is_some());
            s.after_tx();
        }
        assert!(s.outgoing_rv().is_none(), "the budget is spent");
        assert!(s.stalled(), "and that is what stalled() means");

        assert!(s.resume_cq_run(), "a CQ run may be resumed");
        assert!(s.outgoing_rv().is_some(), "and it calls again");
        assert_eq!(
            s.state,
            State::CallingCq,
            "still a CQ run — not restarted elsewhere"
        );
        // The full budget is back, not one call: a pause buys another RUN.
        for _ in 0..2 {
            s.after_tx();
            assert!(s.outgoing_rv().is_some());
        }
    }

    /// ⚠️ THE RESTRICTION IS THE SAFETY. A directed step's budget exists to stop calling a
    /// station that has gone silent; handing it back would call a dead station indefinitely,
    /// which is exactly what `directed_max_calls` was added to prevent.
    #[test]
    fn a_directed_call_budget_is_never_handed_back() {
        for state in [State::AwaitReport, State::AwaitRoger, State::AwaitRr73] {
            let mut s = Station::calling_cq("KD9TAW", "EN52");
            s.state = state;
            s.call_cap = Some(2);
            s.tx_count = 2;
            assert!(!s.resume_cq_run(), "{state:?} must refuse");
            assert_eq!(s.tx_count, 2, "{state:?} keeps its spent budget");
        }
        // Control: the one state it DOES serve still works, so the guard above is
        // discriminating by state rather than refusing everything.
        let mut cq = Station::calling_cq("KD9TAW", "EN52");
        cq.tx_count = 5;
        assert!(cq.resume_cq_run());
        assert_eq!(cq.tx_count, 0);
    }

    #[test]
    fn running_cq_stops_after_its_call_budget() {
        // The OPT-IN budget: with cq_call_cap set, a CQ stops after that many
        // calls — the operator re-arms to call again. (Stock = uncapped, below.)
        let mut s = Station::calling_cq(ME, MY_GRID);
        s.cq_call_cap = Some(MAX_TX_PER_STEP);
        for _ in 0..MAX_TX_PER_STEP {
            assert!(s.outgoing_rv().is_some(), "CQ calls within the budget");
            s.after_tx();
        }
        assert!(s.outgoing_rv().is_none(), "CQ stops after its budget");
        assert!(
            s.stalled(),
            "a finished CQ reports stalled (Resend re-arms)"
        );
    }

    #[test]
    fn uncapped_cq_repeats_indefinitely_like_stock_wsjtx() {
        // Stock WSJT-X: a CQ run repeats until the operator stops it or the Tx
        // watchdog trips — it never self-stalls. (Default: cq_call_cap = None.)
        let mut s = Station::calling_cq(ME, MY_GRID);
        for _ in 0..(MAX_TX_PER_STEP * 5) {
            assert!(s.outgoing_rv().is_some(), "CQ keeps calling (stock)");
            assert!(!s.stalled(), "an uncapped CQ never stalls");
            s.after_tx();
        }
        assert_eq!(s.pending_text().as_deref(), Some("CQ KD9TAW EN61"));
    }

    #[test]
    fn calling_a_station_repeats_indefinitely() {
        // A station you're working (here: answering — sending your grid, awaiting
        // their report) keeps calling FAR past the CQ budget, until they respond or
        // the Tx watchdog stops it.
        let mut s = Station::answering(ME, MY_GRID, DX);
        for _ in 0..(MAX_TX_PER_STEP * 3 + 5) {
            assert!(s.outgoing_rv().is_some(), "keeps calling the station");
            assert!(!s.stalled(), "calling a station never auto-stalls");
            s.after_tx();
        }
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW EN61"));
    }

    #[test]
    fn capped_directed_call_stops_after_its_budget() {
        // With `call_cap` set (the engine defaults it to Some(8)), a directed step the
        // partner never advances STOPS after the budget instead of calling forever — the
        // fix for "endless recalling a station that went silent" in FT8/FT4 S&P. A normal
        // QSO never trips it because each step advances (resetting tx_count) within a few
        // overs; only a stuck/unanswered step accumulates to the cap.
        let mut s = Station::answering(ME, MY_GRID, DX); // AwaitReport: sending my grid
        s.call_cap = Some(4);
        for _ in 0..4 {
            assert!(s.outgoing_rv().is_some(), "calls up to the budget");
            assert!(!s.stalled(), "not stalled before the budget");
            s.after_tx();
        }
        assert!(
            s.outgoing_rv().is_none(),
            "a capped directed call stops after its budget"
        );
        assert!(s.stalled(), "a capped directed call reports stalled");
        // Operator Resend re-arms the step (resets the count) so it tries again.
        s.resend();
        assert!(!s.stalled(), "Resend clears the directed-call stall");
        assert!(s.outgoing_rv().is_some(), "and it calls again");
    }

    /// **D3.** A `/P` operator on the PLAIN Type 1/2 path must never roger a report
    /// nobody sent. The grid-less floor arm was gated on `degraded_vocabulary`, which is
    /// merely "either call has a slash" once `is_compound_qso` is in it — true for pairs
    /// the plain forms carry perfectly, grid and number and all. So a `/P` station that
    /// simply had not copied our report yet, and re-sent its grid, got an `R+nn` back
    /// acknowledging a number it had never transmitted.
    #[test]
    fn a_portable_pair_on_the_plain_path_never_rogers_a_report_nobody_sent() {
        // KD9TAW/P × W9XYZ: both standard, no suffix conflict, nothing hashed — the pair
        // rides Type 1/2 with grid and number intact. A grid-LESS call is still ordinary
        // here: a station with no locator set sends `KD9TAW/P W9XYZ` in the clear, and
        // upstream does the same. It is a CALL, not a roger-worthy answer.
        let mut s = Station::calling_cq("KD9TAW/P", MY_GRID);
        s.observe(&[dec("KD9TAW/P W9XYZ", -7)]);
        assert_eq!(s.state, State::AwaitRoger, "we answered with our report");
        assert_eq!(s.pending_text().as_deref(), Some("W9XYZ KD9TAW/P -07"));

        // They call again — they have not copied our report. The honest reply is the
        // report again, not a roger for a number that was never sent.
        s.observe(&[dec("KD9TAW/P W9XYZ", -7)]);
        assert_eq!(s.state, State::AwaitRoger, "still awaiting their roger");
        assert_eq!(
            s.pending_text().as_deref(),
            Some("W9XYZ KD9TAW/P -07"),
            "a plain pair re-sends the report; it must not roger"
        );
        assert!(s.rx_report.is_none(), "and no number was invented");

        // The pair the packer really cannot express still advances on the same over —
        // there `RRR` is all the frame HAS, so the grid-less form is the DX's answer.
        let mut d = Station::calling_cq("KD9TAW/P", MY_GRID);
        d.observe(&[dec("<KD9TAW/P> F4CYH/R", -7)]);
        assert_eq!(d.state, State::AwaitRoger);
        d.observe(&[dec("<KD9TAW/P> F4CYH/R", -7)]);
        assert_eq!(
            d.pending_text().as_deref(),
            Some("<F4CYH/R> KD9TAW/P RRR"),
            "the degraded floor still rogers"
        );
    }

    /// **D4.** A roger the packer renders as a bare `RRR` is the CLOSING roger — the
    /// partner takes it as one and signs 73 — so it must leave us in `Confirming` (the
    /// initiator's seat, which the app logs from once the over is on the air), not in
    /// `AwaitRr73` waiting for an RR73 nobody will send.
    #[test]
    fn a_roger_that_degrades_to_rrr_leaves_us_confirming_not_awaiting() {
        // Degraded: our own call cannot carry a number beside the hash.
        let mut d = Station::calling_cq("KD9TAW/P", MY_GRID);
        d.observe(&[dec("<KD9TAW/P> F4CYH/R", -7)]); // their call → we "report"
        d.observe(&[dec("<KD9TAW/P> F4CYH/R", -7)]); // their answer → we roger
        assert_eq!(d.pending_text().as_deref(), Some("<F4CYH/R> KD9TAW/P RRR"));
        assert_eq!(
            d.state,
            State::Confirming,
            "a bare RRR closes the QSO for the partner; our state must say so"
        );
        // And the closing over still finishes us, whichever of the three words it is.
        d.observe(&[dec("<KD9TAW/P> F4CYH/R 73", -7)]);
        assert_eq!(d.state, State::Done);

        // Plain pair: the roger carries a number, so it really is Tx3 and we really are
        // waiting for the RR73. Unchanged.
        let mut p = Station::calling_cq(ME, MY_GRID);
        p.observe(&[dec("KD9TAW W9XYZ -09", -7)]);
        assert_eq!(p.pending_text().as_deref(), Some("W9XYZ KD9TAW R-07"));
        assert_eq!(p.state, State::AwaitRr73, "a numeric roger is still Tx3");
    }

    /// The `(AwaitRoger, Report)` arm, pinned. It is NOT dead — deleting it leaves the
    /// class-pair sweep passing (the exchange finds a longer route and the sweep's over
    /// budget is exactly the longer route's length), but the two quadrants whose CQ
    /// runner is nonstandard go from six overs to eight. What it does: we and the DX
    /// both sent Tx2, because once a pair falls to i3=4 a call and a report pack to the
    /// same bytes and they read ours as a call. Theirs is the number that got through —
    /// capture it and roger it rather than trading reports.
    #[test]
    fn a_report_while_awaiting_a_roger_is_captured_and_rogered() {
        let mut s = Station::calling_cq("PJ4/K1ABC", MY_GRID);
        s.observe(&[dec("<PJ4/K1ABC> KD9TAW EN52", -7)]); // they answered with their grid
        assert_eq!(s.state, State::AwaitRoger);
        assert_eq!(s.pending_text().as_deref(), Some("<KD9TAW> PJ4/K1ABC"));

        // They answer our (blank) report with a REPORT of their own — a standard c28
        // sender can still carry a number beside our hash.
        s.observe(&[dec("<PJ4/K1ABC> KD9TAW -09", -7)]);
        assert_eq!(
            s.rx_report,
            Some(-9),
            "their number is the one that got through"
        );
        assert_eq!(
            s.pending_text().as_deref(),
            Some("<KD9TAW> PJ4/K1ABC RRR"),
            "roger it — do not send a third identical blank"
        );
        // Without this arm we would still be at AwaitRoger re-sending the same over.
        assert_ne!(s.state, State::AwaitRoger);
    }

    /// The `(AwaitReport, Grid)` arm is ungated: upstream answers a grid addressed to it
    /// with a report REGARDLESS of state. Gated to the degraded vocabulary, two PLAIN
    /// stations that each answered the other's CQ both sat in `AwaitReport` believing the
    /// other owed the report, and traded grids for as long as anyone watched.
    #[test]
    fn two_stations_that_each_called_the_other_still_finish_the_qso() {
        let mut a = Station::answering(ME, MY_GRID, DX);
        let mut b = Station::answering(DX, "FN31", ME);
        assert_eq!(a.state, State::AwaitReport);
        assert_eq!(b.state, State::AwaitReport);

        // Half-duplex: whoever decodes first reports, and the other rogers it. The tie
        // cannot cross, because only one of them transmits per slot.
        let over = b.pending_text().unwrap();
        a.observe(&[dec(&over, -7)]);
        assert_eq!(a.state, State::AwaitRoger, "a grid to me is answered");
        assert_eq!(a.pending_text().as_deref(), Some("W9XYZ KD9TAW -07"));

        let over = a.pending_text().unwrap();
        b.observe(&[dec(&over, -9)]);
        assert_eq!(b.state, State::AwaitRr73);
        let over = b.pending_text().unwrap();
        a.observe(&[dec(&over, -7)]);
        assert_eq!(a.state, State::Confirming);
        let over = a.pending_text().unwrap();
        b.observe(&[dec(&over, -9)]);
        assert_eq!(
            b.state,
            State::Done,
            "the QSO finishes instead of livelocking"
        );
    }
}

#[cfg(test)]
mod harq_seq_tests {
    use super::*;
    use crate::message::Msg;

    fn decode(text: &str) -> Decode {
        Decode {
            message: text.into(),
            sync: 1.0,
            snr: 0,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    #[test]
    fn rv_escalates_on_unacknowledged_retransmits() {
        let mut s = Station::calling_cq("W9XYZ", "EN37");
        assert_eq!(s.outgoing_rv().unwrap().1, 0, "initial TX is RV0");
        s.after_tx();
        assert_eq!(
            s.outgoing_rv().unwrap().1,
            1,
            "1st unacked retransmit -> RV1"
        );
        s.after_tx();
        assert_eq!(s.outgoing_rv().unwrap().1, 2, "2nd -> RV2");
        s.after_tx();
        assert_eq!(
            s.outgoing_rv().unwrap().1,
            0,
            "3rd wraps to RV0 (fresh HARQ cycle)"
        );
    }

    #[test]
    fn rv_resets_when_partner_advances() {
        let mut s = Station::calling_cq("W9XYZ", "EN37");
        s.after_tx();
        s.after_tx(); // escalate to RV2
        assert_eq!(s.outgoing_rv().unwrap().1, 2);
        // Partner answers our CQ with a grid addressed to us -> the step advances
        // (implicit ACK of our CQ).
        let reply = Msg::Grid {
            to: "W9XYZ".into(),
            de: "K2DEF".into(),
            grid: "FN31".into(),
        };
        s.observe(&[decode(&reply.to_text())]);
        assert_eq!(s.state, State::AwaitRoger, "advanced to the next step");
        assert_eq!(
            s.outgoing_rv().unwrap().1,
            0,
            "RV resets to 0 on implicit ACK"
        );
        assert_eq!(s.tx_count, 0, "TX counter resets on advance");
    }

    #[test]
    fn step_stalls_after_max_tx_without_ack() {
        // A CAPPED CQ stops after its call budget (the only step that auto-stalls).
        let mut s = Station::calling_cq("W9XYZ", "EN37");
        s.cq_call_cap = Some(MAX_TX_PER_STEP);
        for i in 0..MAX_TX_PER_STEP {
            assert!(
                s.outgoing_rv().is_some(),
                "TX {i} of the step should be allowed"
            );
            s.after_tx();
        }
        assert!(
            s.outgoing_rv().is_none(),
            "step exhausted -> withhold further TX"
        );
        assert!(s.stalled(), "stalled() true once the step hits the TX cap");
    }

    #[test]
    fn resend_clears_a_stall_and_re_arms() {
        let mut s = Station::calling_cq("W9XYZ", "EN37");
        s.cq_call_cap = Some(MAX_TX_PER_STEP);
        for _ in 0..MAX_TX_PER_STEP {
            s.after_tx();
        }
        assert!(s.stalled(), "step exhausted");
        assert!(s.outgoing_rv().is_none(), "withheld while stalled");
        s.resend();
        assert!(!s.stalled(), "resend clears the stall");
        assert_eq!(
            s.outgoing_rv().map(|(_, rv)| rv),
            Some(0),
            "resend re-arms at RV0"
        );
        assert_eq!(s.pending_text().as_deref(), Some("CQ W9XYZ EN37"));
    }

    #[test]
    fn override_next_swaps_message_and_resets_cycle() {
        let mut s = Station::calling_cq("W9XYZ", "EN37");
        s.after_tx();
        s.after_tx(); // escalate
        let free = Msg::Other("K2DEF W9XYZ GL OM".into());
        s.override_next(free.clone());
        assert_eq!(s.pending_text().as_deref(), Some("K2DEF W9XYZ GL OM"));
        assert_eq!(s.outgoing_rv().unwrap().1, 0, "override starts fresh HARQ");
        assert_eq!(s.tx_count, 0);
    }

    #[test]
    fn confirm_with_rrr_sends_rrr_not_rr73() {
        // Initiator who prefers a bare RRR: after CQ → grid → report → R-report,
        // the roger message is RRR instead of RR73.
        let mut s = Station::calling_cq("W9XYZ", "EN37");
        s.confirm_with_rrr = true;
        s.observe(&[decode("W9XYZ K2DEF FN31")]); // grid reply → sends report
        assert_eq!(s.state, State::AwaitRoger);
        s.observe(&[decode("W9XYZ K2DEF R-12")]); // R-report → roger
        assert_eq!(s.state, State::Confirming);
        assert!(
            matches!(s.pending, Some(Msg::Rrr { .. })),
            "prefers RRR, got {:?}",
            s.pending
        );
        // Default (RR73) for contrast.
        let mut d = Station::calling_cq("W9XYZ", "EN37");
        d.observe(&[decode("W9XYZ K2DEF FN31")]);
        d.observe(&[decode("W9XYZ K2DEF R-12")]);
        assert!(matches!(d.pending, Some(Msg::Rr73 { .. })), "default RR73");
    }

    #[test]
    fn implicit_nak_does_not_reset_escalation() {
        // Only a genuine step advance resets RV. An unrelated decode (someone
        // else's CQ, or noise) is an implicit NAK and must keep escalating.
        let mut s = Station::calling_cq("W9XYZ", "EN37");
        s.after_tx(); // -> RV1
        assert_eq!(s.outgoing_rv().unwrap().1, 1);
        s.observe(&[decode("CQ N0XYZ FN20")]); // not addressed to us
        assert_eq!(s.state, State::CallingCq, "no advance");
        assert_eq!(
            s.outgoing_rv().unwrap().1,
            1,
            "RV unchanged (still awaiting ACK)"
        );
    }
}
