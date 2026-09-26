//! ⭐ WHETHER A RADIO HAS A SECOND RECEIVER — and, when it does, how the two may be paired.
//!
//! ⛔ **THE ONE THING THIS MODULE EXISTS TO SAY: A HAMLIB CAPABILITY MAP CANNOT ANSWER THIS.**
//! `\dump_state`'s VFO list says what can be ADDRESSED. Whether the radio can RECEIVE on both
//! at once is a fact about the radio, and the only source that settles it is the
//! manufacturer's. The two answers disagree on three of the ten models that expose a Main/Sub
//! VFO list, and they disagree in BOTH directions:
//!
//! - **Over-reports.** An FTDX3000 has a Sub VFO and ONE receiver — Yaesu's manual says its
//!   `[(VFO-B)RX]` button "switches the receiving frequency to VFO-B", where the FTDX5000's
//!   identically-named button "engage[s] the VFO-B receiver … receiving on the two
//!   frequencies". Same control name, same manufacturer, two different machines. An IC-7600
//!   and an IC-756PROIII hear two frequencies at once but share one RF front end.
//! - **Under-reports.** The IC-9100 was absent from the VFO-list probe and Icom's own feature
//!   list opens with "Independent dual receivers in one radio".
//!
//! So the table below is transcribed from vendor documentation, one citation per model, and a
//! model nobody has read a manual for is [`DualRx::Unknown`] — never "no". This is the same
//! discipline `tempo_audio::civ::commands::attenuator_steps_db` already keeps for pad ladders,
//! and the same reason: a made-up capability is acted on at the radio, not merely displayed.
//!
//! ## Three-state, like the cockpit's
//! [`CapState`] mirrors `rigControls.ts`'s `CapState` deliberately — PRESENT / ABSENT /
//! UNKNOWN, and **collapsing UNKNOWN into ABSENT is forbidden**. An unprobed radio has not
//! told us it lacks a second receiver; it has told us nothing.
//!
//! ## Unknown fails OPEN on the pairing, and CLOSED on the offer
//! Two different questions, two different defaults, and the asymmetry is deliberate:
//! - **"Should Nexus offer a Sub for this radio?"** Unknown ⇒ **no**. Offering a receiver that
//!   may not exist puts a dead meter on the screen.
//! - **"May the Sub be tuned here?"** Unknown ⇒ **[`Pairing::Unknown`], which the caller must
//!   treat as permission to try** — command it and let the rig answer. This is
//!   `tempo_audio::rig::Rig::read_rx_ranges`'s rule ("Unknown must always FAIL OPEN … a capability
//!   probe that guessed 'not covered' would block legitimate QSYs") and it holds here for the
//!   same reason.
//!
//! ## ⭐ Per RECEIVER, not per radio
//! [`stage_on`] answers the second half, and it is a different question: two receivers do not
//! entail two of everything behind them. The IC-910(H)'s DSP is an owner-installed option with
//! two sockets, so whether its SUB has one cannot be read from a model number at all — while a
//! capability mask held per RADIO would hand Main's answer to the Sub and advertise a control
//! it may not have. Hence the [`ReceiverId`] key.
//!
//! ⛔ And [`stage_on`] answers ATTRIBUTION — *which receiver may be credited with a stage* —
//! never EXISTENCE. What the radio has is the radio's own `\dump_state` to say; this module
//! never guesses it.
//!
//! Nothing in this module performs I/O or reaches a radio; it is a lookup table and a
//! predicate over it.
//!
//! It lives in `tempo-app` so the engine can consult it: the engine cannot depend on
//! `tempo-audio`, whose native CI-V broker reaches this module as `tempo_audio::dualrx` through
//! a re-export.

use crate::bandplan::band_for_dial;

/// What a radio's SECOND RECEIVER actually is — the three architectures a Main/Sub VFO list
/// cannot tell apart, plus the honest fourth state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DualRx {
    /// **Two independent receivers.** Separate front ends; either receiver may sit wherever it
    /// covers (subject to [`BandPairing`]). Every per-receiver control is genuinely per
    /// receiver.
    Independent,
    /// **One shared RF front end, two audio streams.** The operator really does hear two
    /// frequencies at once, but both must be in the same band because one RF bandpass filter
    /// is tuned to the main readout — and that one filter, preamp and pad serve both.
    ///
    /// ⚠️ This variant is the reason the model is not a boolean. A naive "has a Sub" flag would
    /// offer a per-receiver attenuator that this radio physically does not have.
    SharedFrontEnd,
    /// **One receiver.** A second addressable VFO is a frequency the one receiver can be moved
    /// to, not a second thing listening.
    Single,
    /// No vendor statement has been read for this model. **Not a "no".**
    Unknown,
}

impl DualRx {
    /// Do the two receivers SHARE one RF front end (and therefore one preamp, pad and AGC)?
    /// `None` = unknown, or there is no second receiver to share with.
    pub fn front_end_is_shared(self) -> Option<bool> {
        match self {
            DualRx::Independent => Some(false),
            DualRx::SharedFrontEnd => Some(true),
            DualRx::Single | DualRx::Unknown => None,
        }
    }
}

/// The three-state capability answer, mirroring `rigControls.ts`'s `CapState` so the cockpit's
/// existing rule ("never collapse UNKNOWN into ABSENT") carries across the wire unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapState {
    /// The radio has a second receiver of SOME architecture.
    Present,
    /// The radio has one receiver. A positive, vendor-sourced fact.
    Absent,
    /// Nothing is known. Never render this as "your radio does not have it".
    Unknown,
}

/// One band group in a radio's coverage partition, for [`BandPairing::DistinctGroups`].
///
/// ⚠️ The ranges are deliberately WIDER than any licensed band edge. The question here is only
/// "are these two dials in the same group the manufacturer partitions by", and the licensed
/// edges differ by region (an IC-9700's 430 MHz band is 430–450 in the US and 430–440 in
/// Europe). Encoding region edges would answer a question nobody asked and get it wrong
/// somewhere.
///
/// No `Eq`: the brackets are `f64`. Groups are compared by [`BandGroup::name`], which is the
/// identity that matters here anyway.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandGroup {
    /// The group's name in the manufacturer's own words, for the refusal sentence.
    pub name: &'static str,
    /// Inclusive-low, exclusive-high MHz brackets.
    pub ranges: &'static [(f64, f64)],
}

impl BandGroup {
    fn covers(&self, mhz: f64) -> bool {
        self.ranges.iter().any(|&(lo, hi)| mhz >= lo && mhz < hi)
    }
}

const G_HF50: BandGroup = BandGroup {
    name: "HF/50 MHz",
    ranges: &[(0.03, 60.0)],
};
const G_144: BandGroup = BandGroup {
    name: "144 MHz",
    ranges: &[(100.0, 200.0)],
};
const G_430: BandGroup = BandGroup {
    name: "430 MHz",
    ranges: &[(400.0, 500.0)],
};
const G_1200: BandGroup = BandGroup {
    name: "1200 MHz",
    ranges: &[(1200.0, 1400.0)],
};

/// The IC-9700's three bands.
const GROUPS_VU12: &[BandGroup] = &[G_144, G_430, G_1200];
/// The IC-9100's four — the same three with HF/50 added.
const GROUPS_HF_VU12: &[BandGroup] = &[G_HF50, G_144, G_430, G_1200];

/// How a radio's two receivers may be paired — the constraint BETWEEN them, which no
/// per-receiver coverage list can express.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BandPairing {
    /// Either receiver anywhere it covers; the two do not constrain each other.
    Unrestricted,
    /// The radio's coverage is partitioned and the two receivers must occupy **different**
    /// groups. Icom states this in as many words for two models — see the table.
    DistinctGroups(&'static [BandGroup]),
    /// Both receivers must be in the **same** amateur band.
    SameBand,
    /// No vendor statement. Fails OPEN — see the module note.
    Unknown,
}

/// The answer to "may the Sub sit here while Main sits there".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pairing {
    /// Vendor documentation says this pairing is fine.
    Allowed,
    /// Vendor documentation says it is not, and why — a sentence the UI may show.
    Refused(&'static str),
    /// Nothing is known. **The caller must try it and let the rig answer** — never refuse on
    /// this.
    Unknown,
}

/// ⭐ THE VENDOR TABLE. One row per model a manual has actually been read for; everything else
/// falls through to [`DualRx::Unknown`].
///
/// The `model` key is the Hamlib model number, matching `tempo_audio::rigmodels::rig_models`.
/// Each arm carries the manufacturer's own words in a comment, because the next person to
/// touch a row needs to know whether they are correcting a transcription or overriding a
/// vendor.
pub fn dual_rx(model: u32) -> DualRx {
    match model {
        // ── Two independent receivers ──────────────────────────────────────────────────
        // IC-7610 — Icom: "Independent Dual Receiver"; "Two separate DIGI-SEL preselectors,
        // two separate Band Pass Filter networks, feed two separate A/D converters".
        3078 => DualRx::Independent,
        // IC-9700 — Icom basic manual, "Dualwatch operation": "The IC-9700 has 2 independent
        // receiver circuits, the Main and Sub bands, so that you can use Dualwatch with no
        // compromises, even on different bands and modes."
        3081 => DualRx::Independent,
        // IC-9100 — Icom instruction manual, FEATURES: "Independent dual receivers in one
        // radio; receives two different bands simultaneously."
        // ⚠️ This model was MISSING from the Hamlib VFO-list probe that scoped this
        // programme. It is in the curated catalogue and it genuinely has two receivers.
        3068 => DualRx::Independent,
        // IC-910(H) — Icom instruction manual, "MAIN and SUB bands": "Simultaneous receive on
        // both the MAIN and SUB bands is possible, however the transmission can only be
        // transmitted on the MAIN band — not on the SUB band."
        3044 => DualRx::Independent,
        // FTDX101D / FTDX101MP — Yaesu operation manual (one manual covers both): "The MAIN
        // band receiver … and the SUB band receiver … are completely independent dual
        // receivers, with separate circuit configurations".
        1040 | 1044 => DualRx::Independent,
        // TS-990S — Kenwood brochure: "The TS-990S comes equipped with dual receivers for
        // simultaneous reception on different bands."
        2039 => DualRx::Independent,
        // FTDX5000 — Yaesu: "Dual Receives are built into every FTDX5000"; "capable of
        // simultaneous reception, using the VFO-A and VFO-B receivers".
        // ⚠️ The SECOND RECEIVER is not in doubt; the BAND question is — see `band_pairing`.
        1032 => DualRx::Independent,

        // ── One shared front end, two audio streams ────────────────────────────────────
        // IC-7600 and IC-756PROIII — Icom instruction manuals, verbatim identical in both:
        // "Dualwatch monitors 2 frequencies with the same mode simultaneously. During
        // dualwatch, both frequencies should be on the same band, because the bandpass filter
        // in the RF circuit is selected for the main readout frequency."
        3063 | 3057 => DualRx::SharedFrontEnd,

        // ── One receiver ───────────────────────────────────────────────────────────────
        // FTDX3000 — Yaesu operating manual: "[(VFO-B)RX] Indicator/Switch — This button
        // SWITCHES THE RECEIVING FREQUENCY to VFO-B". The manual contains no "dual receive",
        // "sub receiver" or "dualwatch" anywhere; the absence was checked against a positive
        // control on the same document.
        1037 => DualRx::Single,

        // ⚠️ IC-756PRO (3027) and IC-756PROII (3047) are in the curated catalogue and are
        // very probably the same shared-front-end architecture as the PROIII, but no manual
        // has been read for either. They stay Unknown until one is: a guess here would claim
        // a capability at the radio.
        _ => DualRx::Unknown,
    }
}

/// Does this radio have a second receiver at all, in three states.
///
/// ⚠️ This is the HONEST answer, not the product's offer. A shared-front-end radio really does
/// have a second receiver, so saying `Absent` about one would be the same lie
/// `rigControls.ts`'s pane-foot line is built to avoid — "not on this radio" about something
/// the radio has. Whether Nexus OFFERS it is [`sub_receiver_offered`].
pub fn sub_receiver(model: u32) -> CapState {
    match dual_rx(model) {
        DualRx::Independent | DualRx::SharedFrontEnd => CapState::Present,
        DualRx::Single => CapState::Absent,
        DualRx::Unknown => CapState::Unknown,
    }
}

/// ⛔ DOES THIS BUILD OFFER A SUB RECEIVER FOR THIS RADIO? The product-scope gate, kept as one
/// named function so widening it later is a one-line change rather than an archaeology
/// exercise.
///
/// **v1 offers independent receivers only.** A shared-front-end radio is deliberately excluded
/// even though [`sub_receiver`] reports `Present` for it: its Sub cannot leave the band and
/// shares one preamp, pad and AGC with Main, so every per-receiver control the cockpit would
/// draw for it would be a duplicate of Main's. Supporting it is a real feature with its own
/// rules, not a free consequence of this one.
///
/// `Unknown` is excluded too, and that is the opposite default from [`may_pair`] — see the
/// module note on why the two questions fail in opposite directions.
pub fn sub_receiver_offered(model: u32) -> bool {
    matches!(dual_rx(model), DualRx::Independent)
}

/// The constraint BETWEEN the two receivers, where a manufacturer states one.
pub fn band_pairing(model: u32) -> BandPairing {
    match model {
        // IC-9700 — Icom basic manual, band stacking registers: "The same band cannot be set
        // to both Main and Sub bands." (The same page also notes the weaker rule that the
        // same FREQUENCY cannot be set on both; the BAND rule is the binding one.)
        3081 => BandPairing::DistinctGroups(GROUPS_VU12),
        // IC-9100 — Icom instruction manual, "Selecting a frequency band": "The frequency
        // band, selected in either the MAIN or SUB Band, cannot be selected on the other
        // Band. For example, if the MAIN Band is set to operate on any frequency within the
        // HF/50MHz band, the SUB Band can simultaneously receive on only the 144 MHz, 430 MHz
        // and 1200 MHz frequency bands, or visa versa."
        3068 => BandPairing::DistinctGroups(GROUPS_HF_VU12),

        // FTDX5000 — ⚠️ THE VENDOR SOURCE CONTRADICTS ITSELF. Yaesu's introduction calls the
        // Sub "used for monitoring within the same band as the Main receiver"; the Dual
        // Receive procedure four pages later says "press the [BAND] buttons to select the
        // operating band for the VFO-B receiver". Ruled same-band until someone can put one
        // on a bench: a refused band change is a worse experience than a capability we did
        // not offer. NEEDS-BENCH.
        1032 => BandPairing::SameBand,

        // The shared-front-end pair, by the sentence quoted in `dual_rx`: "both frequencies
        // should be on the same band". Not reachable while `sub_receiver_offered` excludes
        // them, and recorded anyway so the row is right the day it is.
        3063 | 3057 => BandPairing::SameBand,

        // Full-coverage dual-receiver rigs whose manuals state no pairing constraint.
        3078 | 1040 | 1044 | 2039 => BandPairing::Unrestricted,

        // ⚠️ IC-910(H) is deliberately NOT here. Its manual says the two bands "can be
        // assigned to the MAIN and SUB bands" and that [M/S] "exchanges" them, which implies
        // one band per role — but it never states the prohibition the way the IC-9100's does,
        // and an implication is not a specification. Unknown, which fails open.
        _ => BandPairing::Unknown,
    }
}

/// Which group of `groups` covers `mhz`, if any.
fn group_of(groups: &'static [BandGroup], mhz: f64) -> Option<&'static BandGroup> {
    groups.iter().find(|g| g.covers(mhz))
}

/// ⭐ MAY THE SUB SIT AT `sub_mhz` WHILE MAIN SITS AT `main_mhz`?
///
/// ⚠️ [`Pairing::Unknown`] IS NOT A REFUSAL. The caller commands the move and lets the radio
/// answer; refusing on an unknown would block a legitimate retune on every radio nobody has
/// read a manual for, which is most of them. Only [`Pairing::Refused`] may stop anything.
pub fn may_pair(model: u32, main_mhz: f64, sub_mhz: f64) -> Pairing {
    match band_pairing(model) {
        BandPairing::Unrestricted => Pairing::Allowed,
        BandPairing::Unknown => Pairing::Unknown,
        BandPairing::DistinctGroups(groups) => {
            match (group_of(groups, main_mhz), group_of(groups, sub_mhz)) {
                // A dial outside every group this radio partitions by: we cannot say which
                // group it is in, so we do not say anything. Fails open.
                (None, _) | (_, None) => Pairing::Unknown,
                (Some(a), Some(b)) if a.name == b.name => Pairing::Refused(
                    "this radio cannot put both receivers in the same band — \
                     move the sub receiver to another band",
                ),
                _ => Pairing::Allowed,
            }
        }
        BandPairing::SameBand => match (band_for_dial(main_mhz), band_for_dial(sub_mhz)) {
            (Some(a), Some(b)) if a == b => Pairing::Allowed,
            // Off any ham band on either dial — no band to compare, so no ruling.
            (None, _) | (_, None) => Pairing::Unknown,
            _ => Pairing::Refused("this radio's sub receiver stays in the main receiver's band"),
        },
    }
}

// ── D7 — THE CAPABILITY ANSWER IS PER RECEIVER, NOT PER RADIO ─────────────────────────────

/// WHICH receiver a per-receiver answer is about.
///
/// ⭐ This parameter is the whole of D7. A capability function keyed on the model alone gives
/// one answer for a radio that has two receivers, and the caller then hands Main's answer to
/// the Sub — which is precisely the "a single mask would advertise a control the Sub lacks"
/// failure the ruling names.
///
/// ⚠️ Deliberately NOT called `Receiver`: the engine step of this programme is going to add a
/// `Receiver` struct holding the ~25 per-receiver VALUES (dial, s-meter, AGC, filters…). This
/// names WHICH one; that will hold WHAT it is doing. Two different jobs, two names, no rename
/// later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverId {
    /// The receiver CAT describes by default — the one `\dump_state` reports and the one
    /// `tempo_audio::civ::broker`'s `ensure_main` re-asserts the selection to before any
    /// unqualified read.
    Main,
    /// The second receiver, when [`sub_receiver_offered`] says this build offers one.
    Sub,
}

/// The three parts of a receive chain whose per-receiver independence the vendors actually
/// speak to. Coarser than a control list on purpose: manufacturers document the DSP block or
/// the front end, never "does the Sub have its own ANF".
///
/// The mapping onto the 13 `chain: 'rx'` rows of `ui/src/features/rigControls.ts`, which is
/// what the later cockpit step will join on:
///
/// | Stage | Rows |
/// |---|---|
/// | [`RxStage::FrontEnd`] | ATT · PRE · RF · AGC |
/// | [`RxStage::Dsp`] | BW · NB · NR · NRLVL · ANF · MN · NOTCHF |
/// | [`RxStage::Audio`] | AF · SQL |
///
/// AGC sits with the front end on the vendors' own grouping, not on ours: the sentence that
/// makes an IC-7600 shared-front-end ("the bandpass filter in the RF circuit is selected for
/// the main readout frequency") is the same sentence that gives it one AGC, one preamp and
/// one pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxStage {
    /// Everything ahead of the detector: attenuator, preamp, RF gain, AGC.
    FrontEnd,
    /// The DSP block: filter width, NB, NR, auto and manual notch.
    Dsp,
    /// After the detector: AF gain and squelch.
    Audio,
}

/// ⛔ WHOSE STAGE IS IT — **an attribution answer, never an existence answer.**
///
/// This is the distinction that keeps the module honest, and getting it backwards would
/// manufacture exactly the confident wrong answer the rest of the file is built to prevent:
///
/// - **What the radio HAS** comes from the radio — the `\dump_state` masks and step lists that
///   `RigCapsDto` in `rigControls.ts` is the declared shape for. Not from here.
/// - **WHICH RECEIVER may be credited with it** comes from here.
///
/// So [`StageOwner::Own`] on Main's DSP does not say the radio has a DSP. It says that if it
/// has one, it is Main's. An IC-910(H) with no UT-106 fitted has no DSP on either receiver,
/// and no model number can know that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOwner {
    /// This receiver drives its own. A per-receiver control here is genuinely per receiver.
    Own,
    /// ⚠️ ONE PHYSICAL STAGE SERVES BOTH. Driving it "on the Sub" moves Main's too, because
    /// there is only one. This is the second constraint BETWEEN the receivers in this module —
    /// [`BandPairing`] is the other — and it is why the capability could not be a boolean:
    /// a shared pad is neither present as the Sub's nor absent from the radio.
    SharedWithMain,
    /// No vendor statement, or a capability that is an owner-installed OPTION and therefore
    /// unknowable from a model number.
    ///
    /// ⛔ **NEVER A "NO", and never a reason to suppress anything on its own.** The caller
    /// falls back to what it has always done — the sticky "has this radio ever reported it"
    /// answer `capStateFor`'s UNKNOWN branch already falls back to. Collapsing this into
    /// "the Sub does not have it" is the forbidden collapse wearing per-receiver clothes.
    Unknown,
}

/// ⭐ D7 — MAY `rx` BE CREDITED WITH THIS RADIO'S `stage`?
///
/// Read [`StageOwner`] first: this answers attribution, not existence.
///
/// ⚠️ It does NOT answer "is there a second receiver" — that is [`sub_receiver`], and
/// "should this build offer one" is [`sub_receiver_offered`]. Asking about the Sub of a radio
/// that has none returns [`StageOwner::Unknown`], because there is nothing to attribute.
pub fn stage_on(model: u32, rx: ReceiverId, stage: RxStage) -> StageOwner {
    // Main owns the radio's stages by definition of being the receiver CAT is talking to:
    // `\dump_state` describes it, and `ensure_main` exists to guarantee an unqualified read
    // or write lands on it. Nothing about a second receiver changes that.
    if rx == ReceiverId::Main {
        return StageOwner::Own;
    }
    match stage {
        // ⭐ THE FRONT END NEEDS NO TABLE — it falls straight out of the architecture already
        // classified above, so it can never drift from it. Independent = separate front ends;
        // shared-front-end = one, by the quoted Icom sentence; no Sub or unread = nothing to
        // say.
        RxStage::FrontEnd => match dual_rx(model).front_end_is_shared() {
            Some(true) => StageOwner::SharedWithMain,
            Some(false) => StageOwner::Own,
            None => StageOwner::Unknown,
        },
        // ⚠️ THE OTHER TWO STAGES NEED A TABLE, because the architecture does not imply them.
        // Two independent front ends do not entail two DSP blocks or two AF stages — the
        // IC-910(H) below is the counter-example in the vendor's own words — so anything not
        // documented per receiver stays Unknown, on the same rule as `dual_rx` itself.
        RxStage::Dsp => match model {
            // FTDX101D / MP — Yaesu operation manual: each receiver has "individual notch and
            // noise filters … its own set of adjustable filter widths".
            1040 | 1044 => StageOwner::Own,
            // FTDX5000 — Yaesu, Introduction: "Both VFO-A and VFO-B receivers utilizes DSP
            // filtering."
            1032 => StageOwner::Own,
            // ⛔ IC-910(H) — THE MODEL CASE FOR D7, and the reason a per-radio mask is wrong.
            // The DSP is the OPTIONAL UT-106 and the radio has TWO SOCKETS: "Up to 2 DSP units
            // can be installed for simultaneous DSP operation for both MAIN and SUB bands.
            // When only 1 DSP unit is installed, DSP functions can be operated in either the
            // MAIN or SUB band, whichever is being accessed." So this radio's Sub DSP depends
            // on what its owner bought, which no model number and no `\dump_state` can see.
            // Unknown is the only honest answer, and it must NOT read as Own on the strength
            // of the receivers being independent. The arm is spelled out rather than left to
            // the fallthrough below because the next person to widen this table needs the
            // reason on the row, not a reconstruction of why 910 is not in the line above.
            3044 => StageOwner::Unknown,
            // Everything else: two receivers confirmed, per-receiver DSP never stated.
            _ => StageOwner::Unknown,
        },
        RxStage::Audio => match model {
            // IC-7610 — Icom: "independent AF/RF knobs for the main and sub bands". (The RF
            // half corroborates the front end, which is already Own by architecture.)
            3078 => StageOwner::Own,
            // FTDX101D / MP — separate AF outputs per receiver, same manual as the DSP row.
            1040 | 1044 => StageOwner::Own,
            // FTDX5000 — Yaesu: a separate [AF GAIN] knob per VFO.
            1032 => StageOwner::Own,
            _ => StageOwner::Unknown,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole programme's premise, as one executable guard.
    ///
    /// These three models all expose a Main/Sub VFO list to Hamlib and all three would be
    /// scored as dual-receiver radios by any capability map. None of them may be offered a
    /// Sub. If this test ever goes green by accident it will be because someone re-derived
    /// the capability from the wire, which is the one thing this module exists to prevent.
    #[test]
    fn an_addressable_sub_vfo_is_not_a_second_receiver() {
        // One receiver, full stop.
        assert_eq!(
            dual_rx(1037),
            DualRx::Single,
            "FTDX3000 has a sub VFO and one receiver"
        );
        assert!(
            !sub_receiver_offered(1037),
            "FTDX3000 must never be offered a sub receiver"
        );
        // Two audio streams, one front end — a second receiver, but not one v1 offers.
        for (model, who) in [(3063u32, "IC-7600"), (3057, "IC-756PROIII")] {
            assert_eq!(
                dual_rx(model),
                DualRx::SharedFrontEnd,
                "{who} shares one RF front end"
            );
            assert!(
                !sub_receiver_offered(model),
                "{who} must not be offered a sub receiver in v1"
            );
        }
    }

    /// ⚠️ The discriminator for the test above: a model that IS offered. Without this the
    /// assertions there would pass on an implementation that offered nothing to anybody.
    #[test]
    fn a_genuine_dual_receiver_is_offered() {
        for (model, who) in [
            (3078u32, "IC-7610"),
            (3081, "IC-9700"),
            (3068, "IC-9100"),
            (3044, "IC-910"),
            (1040, "FTDX101D"),
            (1044, "FTDX101MP"),
            (2039, "TS-990S"),
            (1032, "FTDX5000"),
        ] {
            assert_eq!(dual_rx(model), DualRx::Independent, "{who}");
            assert!(sub_receiver_offered(model), "{who} is offered");
            assert_eq!(sub_receiver(model), CapState::Present, "{who}");
        }
    }

    /// UNKNOWN is not ABSENT, and the two must be distinguishable by their own values —
    /// an assertion that only checked "not Present" would pass on either.
    #[test]
    fn an_unread_model_is_unknown_and_a_single_receiver_is_absent() {
        // Nobody has read a manual for these; the catalogue has ~105 more like them.
        for (model, who) in [
            (3073u32, "IC-7300"),
            (3027, "IC-756PRO — probably shared-front-end, but unread"),
            (2036, "FlexRadio"),
            (1, "Hamlib Dummy"),
        ] {
            assert_eq!(dual_rx(model), DualRx::Unknown, "{who}");
            assert_eq!(
                sub_receiver(model),
                CapState::Unknown,
                "{who} — UNKNOWN must never collapse to ABSENT"
            );
            assert!(!sub_receiver_offered(model), "{who} is not offered");
        }
        // …and the positive, vendor-sourced "no", which must NOT read as Unknown.
        assert_eq!(
            sub_receiver(1037),
            CapState::Absent,
            "FTDX3000 is a known no"
        );
        assert_ne!(
            sub_receiver(1037),
            sub_receiver(3073),
            "a known single receiver and an unread model must not answer the same"
        );
    }

    /// The shared-front-end radio's Sub has no preamp, pad or AGC of its own — the trap a
    /// boolean capability walks straight into.
    #[test]
    fn front_end_sharing_separates_the_two_dual_receiver_architectures() {
        assert_eq!(dual_rx(3078).front_end_is_shared(), Some(false), "IC-7610");
        assert_eq!(dual_rx(3063).front_end_is_shared(), Some(true), "IC-7600");
        assert_eq!(dual_rx(1037).front_end_is_shared(), None, "FTDX3000");
        assert_eq!(dual_rx(3073).front_end_is_shared(), None, "unread model");
    }

    /// D6 — the constraint BETWEEN the receivers. The IC-9700 refuses two receivers in one
    /// band; the same call on an unrestricted rig at the same frequencies allows it, which is
    /// what makes this test about the CONSTRAINT and not about the frequencies.
    #[test]
    fn a_distinct_groups_rig_refuses_both_receivers_in_one_band() {
        // Icom: "The same band cannot be set to both Main and Sub bands."
        assert!(
            matches!(may_pair(3081, 144.200, 145.500), Pairing::Refused(_)),
            "IC-9700 both receivers on 144 MHz"
        );
        assert_eq!(
            may_pair(3081, 144.200, 432.100),
            Pairing::Allowed,
            "IC-9700 144 + 430 is the satellite pairing"
        );
        assert_eq!(
            may_pair(3081, 432.100, 1296.100),
            Pairing::Allowed,
            "IC-9700 430 + 1200"
        );
        // ⚠️ THE CONTROL: the identical same-band call on a rig with no pairing rule. If this
        // ever refuses, the refusal above is coming from the frequencies, not the model.
        assert_eq!(
            may_pair(3078, 144.200, 145.500),
            Pairing::Allowed,
            "IC-7610 has no pairing constraint"
        );
        // The IC-9100 partitions HF/50 off as well.
        assert!(
            matches!(may_pair(3068, 14.074, 14.100), Pairing::Refused(_)),
            "IC-9100 both receivers in HF"
        );
        assert_eq!(
            may_pair(3068, 14.074, 144.200),
            Pairing::Allowed,
            "IC-9100 HF + 144 is a legal pair"
        );
    }

    /// D8 — the FTDX5000 is same-band until a bench says otherwise.
    #[test]
    fn the_ftdx5000_sub_stays_in_the_main_band() {
        assert_eq!(
            may_pair(1032, 14.074, 14.200),
            Pairing::Allowed,
            "same band is what this rig is documented for"
        );
        assert!(
            matches!(may_pair(1032, 14.074, 7.100), Pairing::Refused(_)),
            "cross-band is unconfirmed, so it is not offered"
        );
    }

    /// ⛔ UNKNOWN FAILS OPEN. A pairing nobody has documented must never come back as a
    /// refusal — that would block a legitimate retune on most of the catalogue.
    #[test]
    fn an_undocumented_pairing_is_unknown_and_never_a_refusal() {
        for (model, who) in [
            (3044u32, "IC-910 — implied, never stated"),
            (3073, "IC-7300"),
        ] {
            assert_eq!(may_pair(model, 144.200, 145.500), Pairing::Unknown, "{who}");
        }
        // A dial outside every group the radio partitions by is also "cannot say", not "no".
        assert_eq!(
            may_pair(3081, 14.074, 144.200),
            Pairing::Unknown,
            "IC-9700 asked about an HF dial it has no group for"
        );
    }

    /// The group brackets must not care which region's band edges a radio was sold with.
    #[test]
    fn band_groups_span_every_regional_variant() {
        // 430 MHz: 430–440 in Europe, 430–450 in the US — both land in one group.
        assert_eq!(
            may_pair(3081, 435.000, 145.900),
            Pairing::Allowed,
            "European 430 edge"
        );
        assert_eq!(
            may_pair(3081, 449.000, 145.900),
            Pairing::Allowed,
            "US 430 edge"
        );
        assert!(
            matches!(may_pair(3081, 435.000, 449.000), Pairing::Refused(_)),
            "both European and US 430 dials are the SAME group"
        );
    }

    // ── D7 — per receiver, not per radio ──────────────────────────────────────────────

    /// ⭐ D7 IN ONE ASSERTION: the same radio, the same stage, two receivers, two answers.
    /// A capability function keyed on the model alone cannot make this test pass.
    #[test]
    fn the_same_radio_answers_differently_for_main_and_sub() {
        // IC-910(H) — the optional second DSP unit (see the table).
        assert_eq!(
            stage_on(3044, ReceiverId::Main, RxStage::Dsp),
            StageOwner::Own
        );
        assert_eq!(
            stage_on(3044, ReceiverId::Sub, RxStage::Dsp),
            StageOwner::Unknown
        );
        // IC-7600 — one RF front end between the two receivers.
        assert_eq!(
            stage_on(3063, ReceiverId::Main, RxStage::FrontEnd),
            StageOwner::Own
        );
        assert_eq!(
            stage_on(3063, ReceiverId::Sub, RxStage::FrontEnd),
            StageOwner::SharedWithMain
        );
        // Main is the receiver CAT describes, on every architecture and every stage — the
        // one answer this module gives without consulting anything.
        for model in [
            3078u32, 3081, 3068, 3044, 1040, 1044, 2039, 1032, 3063, 1037, 3073,
        ] {
            for stage in [RxStage::FrontEnd, RxStage::Dsp, RxStage::Audio] {
                assert_eq!(
                    stage_on(model, ReceiverId::Main, stage),
                    StageOwner::Own,
                    "model {model} main {stage:?}"
                );
            }
        }
    }

    /// ⛔ THE IC-910(H) CASE D7 WAS RULED ON — an owner-installed option cannot be read from a
    /// model number, so the Sub's DSP is Unknown even though the receivers are independent.
    #[test]
    fn an_optional_second_dsp_is_unknown_while_a_documented_one_is_owned() {
        assert_eq!(
            stage_on(3044, ReceiverId::Sub, RxStage::Dsp),
            StageOwner::Unknown,
            "IC-910H — the UT-106 is an option and there are two sockets"
        );
        // ⚠️ THE DISCRIMINATOR. Without this, a blanket `Sub => Unknown` would pass above.
        assert_eq!(
            stage_on(1040, ReceiverId::Sub, RxStage::Dsp),
            StageOwner::Own,
            "FTDX101D — individual notch and noise filters per receiver"
        );
        // ⭐ AND THE DIFFERENCE CANNOT BE COMING FROM THE ARCHITECTURE: both radios are
        // classified identically, so only the per-receiver table can separate them.
        assert_eq!(dual_rx(3044), dual_rx(1040), "both are Independent");
        assert_ne!(
            stage_on(3044, ReceiverId::Sub, RxStage::Dsp),
            stage_on(1040, ReceiverId::Sub, RxStage::Dsp),
        );
    }

    /// The shared-front-end Sub has no pad, preamp or AGC of its own — and "shared" must be
    /// distinguishable BY VALUE from "unknown", or the cockpit cannot tell a control that
    /// moves Main from one nobody has documented.
    #[test]
    fn a_shared_front_end_sub_does_not_own_its_pad() {
        for (model, who) in [(3063u32, "IC-7600"), (3057, "IC-756PROIII")] {
            assert_eq!(
                stage_on(model, ReceiverId::Sub, RxStage::FrontEnd),
                StageOwner::SharedWithMain,
                "{who}"
            );
        }
        // ⚠️ CONTROL: the same stage on the same receiver of a rig with two front ends.
        assert_eq!(
            stage_on(3078, ReceiverId::Sub, RxStage::FrontEnd),
            StageOwner::Own,
            "IC-7610 — two separate Band Pass Filter networks"
        );
        assert_ne!(
            stage_on(3063, ReceiverId::Sub, RxStage::FrontEnd),
            stage_on(3073, ReceiverId::Sub, RxStage::FrontEnd),
            "shared must not answer the same as an unread model"
        );
    }

    /// ⭐ THE TEMPTING SHORTCUT, PINNED SHUT: two independent receivers do NOT entail two of
    /// everything behind them. Audio is Own only where a vendor said so.
    #[test]
    fn per_receiver_audio_comes_from_the_vendor_not_the_architecture() {
        assert_eq!(
            stage_on(3078, ReceiverId::Sub, RxStage::Audio),
            StageOwner::Own,
            "IC-7610 — independent AF knobs for the main and sub bands"
        );
        assert_eq!(
            stage_on(3081, ReceiverId::Sub, RxStage::Audio),
            StageOwner::Unknown,
            "IC-9700 — two receivers confirmed, per-receiver AF never stated"
        );
        assert_eq!(dual_rx(3078), dual_rx(3081), "both are Independent");
        assert_ne!(
            stage_on(3078, ReceiverId::Sub, RxStage::Audio),
            stage_on(3081, ReceiverId::Sub, RxStage::Audio),
        );
    }

    /// A radio with one receiver has nothing to attribute to a second one — and this function
    /// is not what stops a Sub being drawn for it.
    #[test]
    fn a_radio_with_one_receiver_has_nothing_to_attribute() {
        for stage in [RxStage::FrontEnd, RxStage::Dsp, RxStage::Audio] {
            assert_eq!(
                stage_on(1037, ReceiverId::Sub, stage),
                StageOwner::Unknown,
                "FTDX3000 {stage:?}"
            );
        }
        // The gate is `sub_receiver` / `sub_receiver_offered`, which answer positively here.
        assert_eq!(sub_receiver(1037), CapState::Absent);
        assert!(!sub_receiver_offered(1037));
    }

    /// D2 ↔ D7 coherence: v1 offers only independent receivers, so every offered Sub owns its
    /// own front end. If category 2 is ever widened into the offer, this fails and points at
    /// the per-receiver controls that would have to be re-thought first.
    #[test]
    fn every_offered_sub_owns_its_own_front_end() {
        for model in [3078u32, 3081, 3068, 3044, 1040, 1044, 2039, 1032] {
            assert!(sub_receiver_offered(model), "model {model}");
            assert_eq!(
                stage_on(model, ReceiverId::Sub, RxStage::FrontEnd),
                StageOwner::Own,
                "model {model}"
            );
        }
        // ⚠️ The discriminator: a radio with a second receiver that v1 does not offer.
        assert!(!sub_receiver_offered(3063));
        assert_eq!(
            stage_on(3063, ReceiverId::Sub, RxStage::FrontEnd),
            StageOwner::SharedWithMain
        );
    }
}
