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
//! discipline `civ::commands::attenuator_steps_db` already keeps for pad ladders, and the same
//! reason: a made-up capability is acted on at the radio, not merely displayed.
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
//!   [`crate::rig::Rig::read_rx_ranges`]'s rule ("Unknown must always FAIL OPEN … a capability
//!   probe that guessed 'not covered' would block legitimate QSYs") and it holds here for the
//!   same reason.
//!
//! Nothing in this module performs I/O or reaches a radio; it is a lookup table and a
//! predicate over it.

use tempo_app::bandplan::band_for_dial;

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
/// The `model` key is the Hamlib model number, matching [`crate::rigmodels::rig_models`].
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
}
