//! ⭐ THE SUB RECEIVER'S CONTROLS — what the operator asks of the Sub, and what the radio took.
//!
//! The Sub's controls reach the radio by ONE road. The cockpit asks here
//! ([`Engine::request_sub_level`]); the radio loop takes the request at receive time, never while
//! anything is keyed, and — only when the CAT path serving the radio can NAME the Sub — sends it
//! as `L Sub <level> <value>`. Nexus's own CI-V daemon turns that into the per-receiver command
//! the radio documents (`29 01 …` on an IC-7610; select Sub, set, select Main on an IC-9700,
//! which has no band-directed form). The loop then says what the radio answered
//! ([`Engine::observe_sub_level`]).
//!
//! ## Three levels, and why only three
//! RF gain (the front end), AF gain and squelch (the audio stage). The Sub is DISPLAY + AUDIO
//! (ruling D4), and AF is the audio. A level is offered only where the capability table credits
//! the Sub with its stage (D7, [`dualrx::stage_on`]) — so an IC-7610's Sub gets all three
//! ("independent AF/RF knobs for the main and sub bands") and an IC-9700's gets RF alone,
//! because no vendor statement gives its Sub an AF stage of its own. Nothing in the DSP stage
//! is here: no vendor statement credits a DSP to the Sub of a radio this build can address.
//!
//! ## What the snapshot says about them
//! The value the RADIO ACCEPTED from Nexus (`RPRT 0`) — never a read-back, because nothing reads
//! the Sub back in this build. A refused value is not shown; the last accepted one stays, so a
//! control the radio would not take springs back to what it did take.
//!
//! ## Whose Sub
//! Everything here belongs to ONE (radio, model). A value, a request or a route learned for one
//! radio is never read for another: a radio switch, or a model edit, starts from nothing known.
//!
//! Nothing here reaches a transmit path. The Sub has no transmitter of its own in this model,
//! these are receive levels, and the loop withholds them while the rig is keyed.

use super::Engine;
use crate::dualrx::{self, ReceiverId, RxStage, StageOwner};

/// A level the Sub's controls can set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubLevel {
    /// RF (receive) gain — the front end.
    Rf,
    /// AF gain — the audio stage; what makes the Sub heard.
    Af,
    /// Squelch — the audio stage.
    Sql,
}

impl SubLevel {
    /// Every level, in the registry's signal order (front end, then audio).
    pub const ALL: [SubLevel; 3] = [SubLevel::Rf, SubLevel::Af, SubLevel::Sql];

    /// The Hamlib level token the radio loop sends, the same one Main's control uses.
    pub fn token(self) -> &'static str {
        match self {
            SubLevel::Rf => "RF",
            SubLevel::Af => "AF",
            SubLevel::Sql => "SQL",
        }
    }

    /// The name the UI sends (`set_sub_level`). Anything else is not a Sub control.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "rf" => Some(SubLevel::Rf),
            "af" => Some(SubLevel::Af),
            "sql" => Some(SubLevel::Sql),
            _ => None,
        }
    }

    /// The receive stage the level belongs to — the key D7's per-receiver answer joins on.
    fn stage(self) -> RxStage {
        match self {
            SubLevel::Rf => RxStage::FrontEnd,
            SubLevel::Af | SubLevel::Sql => RxStage::Audio,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A Sub level the radio loop is to send — and WHOSE Sub it was taken for, so the radio's
/// answer can only ever be credited to that radio (see [`Engine::observe_sub_level`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubLevelWrite {
    owner: (u32, u32),
    pub level: SubLevel,
    pub value: f32,
}

/// The Sub's control state for one (radio id, Hamlib model).
#[derive(Debug, Default)]
pub struct SubControls {
    /// Whose Sub this is. `None` = nothing learned yet.
    owner: Option<(u32, u32)>,
    /// Requests the loop has not taken yet; the latest value wins.
    pending: [Option<f32>; 3],
    /// The last value the radio ACCEPTED for each level. Not a read-back.
    accepted: [Option<f32>; 3],
    /// Can the CAT path serving this radio name the Sub — the radio loop's answer.
    route: Option<bool>,
}

impl SubControls {
    /// This state for `owner`, started from nothing when it belongs to another radio.
    fn for_owner(&mut self, owner: (u32, u32)) -> &mut Self {
        if self.owner != Some(owner) {
            *self = SubControls {
                owner: Some(owner),
                ..SubControls::default()
            };
        }
        self
    }

    /// This state, only if it is `owner`'s.
    fn of(&self, owner: (u32, u32)) -> Option<&Self> {
        (self.owner == Some(owner)).then_some(self)
    }
}

impl Engine {
    /// The radio in play, as the Sub's state is keyed: its id and its Hamlib model.
    fn sub_owner(&self) -> (u32, u32) {
        (self.settings.active_radio, self.settings.rig_model)
    }

    /// Ask for a Sub level (0.0–1.0); the radio loop applies it at receive time.
    ///
    /// REFUSED, with the reason, where it could not reach the Sub: no Sub offered for this radio
    /// (UNKNOWN is never an offer, D3), a stage no vendor statement credits to the Sub (D7), or a
    /// CAT path that cannot name the Sub — including one the loop has not reported yet. A request
    /// that could only fail at the radio is refused here instead of queued.
    pub fn request_sub_level(&mut self, level: SubLevel, frac: f32) -> Result<(), String> {
        let owner = self.sub_owner();
        let model = owner.1;
        if !dualrx::sub_receiver_offered(model) {
            return Err("this radio has no sub receiver Nexus can drive".into());
        }
        if dualrx::stage_on(model, ReceiverId::Sub, level.stage()) != StageOwner::Own {
            return Err(format!(
                "{} is not confirmed for this radio's sub receiver",
                level.token()
            ));
        }
        if !frac.is_finite() {
            return Err("not a level".into());
        }
        let sub = self.sub_controls.for_owner(owner);
        if sub.route != Some(true) {
            return Err(
                "this connection cannot reach the sub receiver — it needs Nexus's own CI-V \
                 control of the radio"
                    .into(),
            );
        }
        sub.pending[level.index()] = Some(frac.clamp(0.0, 1.0));
        Ok(())
    }

    /// The radio loop's side, once per receive-time poll: record whether the CAT path serving
    /// the radio can name the Sub (`names_sub`), and take the requests waiting for it.
    ///
    /// On a path that cannot, the requests are DROPPED rather than kept: nothing will ever carry
    /// them, and the snapshot then says so (`subCommandable: false`), which takes the controls
    /// off the screen.
    pub fn take_sub_level_requests(&mut self, names_sub: bool) -> Vec<SubLevelWrite> {
        let owner = self.sub_owner();
        let sub = self.sub_controls.for_owner(owner);
        sub.route = Some(names_sub);
        let pending = std::mem::take(&mut sub.pending);
        if !names_sub {
            return Vec::new();
        }
        SubLevel::ALL
            .into_iter()
            .filter_map(|level| {
                pending[level.index()].map(|value| SubLevelWrite {
                    owner,
                    level,
                    value,
                })
            })
            .collect()
    }

    /// What the radio answered to a Sub level the loop sent. An accepted value becomes the one
    /// the snapshot shows; a refusal leaves the last accepted value where it was.
    ///
    /// ⚠️ THE ANSWER CARRIES ITS OWNER. The loop writes with the engine lock released, and a
    /// radio switch can land in that window — after which anything touching the Sub's state
    /// re-keys it to the NEW radio. Matching on the owner the write was taken for, not on
    /// whatever the state says now, is what keeps one radio's answer off another's Sub.
    pub fn observe_sub_level(&mut self, write: SubLevelWrite, accepted: bool) {
        if accepted
            && self.sub_owner() == write.owner
            && self.sub_controls.owner == Some(write.owner)
        {
            self.sub_controls.accepted[write.level.index()] = Some(write.value);
        }
    }

    /// Can Nexus command the Sub's controls on the CAT path serving the radio in play —
    /// `None` until the radio loop has said.
    pub fn sub_commandable(&self) -> Option<bool> {
        self.sub_controls.of(self.sub_owner()).and_then(|s| s.route)
    }

    /// The last value the radio accepted for a Sub level — `None` when none has been.
    pub fn sub_level_accepted(&self, level: SubLevel) -> Option<f32> {
        self.sub_controls
            .of(self.sub_owner())
            .and_then(|s| s.accepted[level.index()])
    }

    /// The receivers as the snapshot carries them: the engine's model
    /// ([`Engine::receivers`]), plus whether the Sub's controls can reach it.
    pub fn receivers_dto(&self) -> crate::dto::ReceiversDto {
        let mut dto = crate::dto::ReceiversDto::from(&self.receivers());
        if dto.sub.is_some() {
            dto.sub_commandable = self.sub_commandable();
        }
        dto
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tx_gate_table::station;

    /// What the loop was handed, as (level, value).
    fn taken(e: &mut Engine, names_sub: bool) -> Vec<(SubLevel, f32)> {
        e.take_sub_level_requests(names_sub)
            .into_iter()
            .map(|w| (w.level, w.value))
            .collect()
    }

    /// The loop's whole round: take what is waiting, and report the radio's `answer` to each.
    fn round(e: &mut Engine, answer: bool) -> Vec<(SubLevel, f32)> {
        let writes = e.take_sub_level_requests(true);
        for w in &writes {
            e.observe_sub_level(*w, answer);
        }
        writes.into_iter().map(|w| (w.level, w.value)).collect()
    }

    /// A dual-receiver station whose loop has reported that its CAT path names the Sub.
    fn routed(model: u32) -> Engine {
        let mut e = station(model, "general", "phone", 14.250, "20m", "USB");
        assert!(taken(&mut e, true).is_empty(), "nothing queued yet");
        e
    }

    /// ⭐ A SUB CONTROL GOES TO THE SUB — the loop is handed it with the Sub's token, and Main's
    /// value for the same level never moves. The radio's acceptance is what the Sub then shows.
    #[test]
    fn a_sub_level_goes_to_the_loop_for_the_sub_and_never_touches_main() {
        let mut e = routed(3078); // IC-7610
        e.observe_rig_af_gain(0.9); // Main's reading, which must survive untouched
        e.request_sub_level(SubLevel::Af, 0.25).expect("offered");
        let writes = e.take_sub_level_requests(true);
        assert_eq!(
            writes
                .iter()
                .map(|w| (w.level, w.value))
                .collect::<Vec<_>>(),
            vec![(SubLevel::Af, 0.25)]
        );
        assert_eq!(
            writes[0].level.token(),
            "AF",
            "the Sub's AF, by Main's token"
        );
        assert_eq!(
            e.receivers().sub.expect("offered").af_gain,
            None,
            "not shown before the radio accepted it"
        );
        e.observe_sub_level(writes[0], true);
        let rs = e.receivers();
        assert_eq!(rs.sub.expect("offered").af_gain, Some(0.25));
        assert_eq!(rs.main.af_gain, Some(0.9), "Main's AF is Main's");
        assert_eq!(e.af_gain(), None, "Main's commanded AF was never touched");
        assert_eq!(
            e.snapshot().radio.af_gain,
            Some(0.9),
            "the flat field is Main's"
        );
    }

    /// The latest request wins, a refusal keeps what the radio last took, and one level's
    /// answer never lands on another.
    #[test]
    fn the_latest_request_wins_and_a_refusal_keeps_the_last_accepted() {
        let mut e = routed(3078);
        e.request_sub_level(SubLevel::Rf, 0.25).unwrap();
        e.request_sub_level(SubLevel::Rf, 0.5).unwrap();
        e.request_sub_level(SubLevel::Sql, 0.75).unwrap();
        let writes = e.take_sub_level_requests(true);
        assert_eq!(
            writes
                .iter()
                .map(|w| (w.level, w.value))
                .collect::<Vec<_>>(),
            vec![(SubLevel::Rf, 0.5), (SubLevel::Sql, 0.75)],
            "signal order, latest value"
        );
        e.observe_sub_level(writes[0], true);
        e.observe_sub_level(writes[1], false);
        assert_eq!(e.sub_level_accepted(SubLevel::Rf), Some(0.5));
        assert_eq!(
            e.sub_level_accepted(SubLevel::Sql),
            None,
            "refused: not shown"
        );
        assert_eq!(e.sub_level_accepted(SubLevel::Af), None, "never asked");
        e.request_sub_level(SubLevel::Rf, 0.125).unwrap();
        round(&mut e, false);
        assert_eq!(
            e.sub_level_accepted(SubLevel::Rf),
            Some(0.5),
            "a refusal leaves the last value the radio took"
        );
    }

    /// ⛔ REFUSED WHERE IT CANNOT REACH THE SUB — and a refusal queues nothing.
    #[test]
    fn a_sub_level_is_refused_where_no_sub_can_take_it() {
        // No Sub offered: UNKNOWN (IC-7300), ABSENT (FTDX3000), a shared front end (IC-7600).
        for model in [3073u32, 1037, 3063] {
            let mut e = routed(model);
            assert!(e.request_sub_level(SubLevel::Rf, 0.5).is_err(), "{model}");
            assert!(taken(&mut e, true).is_empty(), "{model}");
        }
        // D7: the IC-9700's Sub has its own front end but no documented audio stage.
        let mut e = routed(3081);
        assert!(
            e.request_sub_level(SubLevel::Rf, 0.5).is_ok(),
            "RF: front end"
        );
        assert!(
            e.request_sub_level(SubLevel::Af, 0.5).is_err(),
            "AF: unknown"
        );
        assert!(
            e.request_sub_level(SubLevel::Sql, 0.5).is_err(),
            "SQL: unknown"
        );
        assert_eq!(taken(&mut e, true), vec![(SubLevel::Rf, 0.5)]);
        // The route: not yet reported, then a path that cannot name the Sub.
        let mut e = station(3078, "general", "phone", 14.250, "20m", "USB");
        assert!(
            e.request_sub_level(SubLevel::Af, 0.5).is_err(),
            "route unknown"
        );
        assert!(taken(&mut e, false).is_empty());
        assert!(e.request_sub_level(SubLevel::Af, 0.5).is_err(), "no route");
        assert_eq!(e.sub_commandable(), Some(false));
        // A non-number is not a level.
        let mut e = routed(3078);
        assert!(e.request_sub_level(SubLevel::Af, f32::NAN).is_err());
    }

    /// A path that stops being able to name the Sub drops what was waiting for it.
    #[test]
    fn a_route_that_cannot_name_the_sub_drops_the_requests() {
        let mut e = routed(3078);
        e.request_sub_level(SubLevel::Af, 0.5).unwrap();
        assert!(taken(&mut e, false).is_empty(), "nothing is sent");
        assert!(
            taken(&mut e, true).is_empty(),
            "…and nothing is kept for later"
        );
    }

    /// ⛔ ONE RADIO'S SUB IS NEVER READ FOR ANOTHER: a model change (or a switch) starts from
    /// nothing known — no route, no accepted value, no request.
    #[test]
    fn a_radio_change_never_carries_one_sub_to_another() {
        let mut e = routed(3078);
        e.request_sub_level(SubLevel::Rf, 0.5).unwrap();
        round(&mut e, true);
        e.request_sub_level(SubLevel::Rf, 0.25).unwrap(); // still pending at the switch
        assert_eq!(e.sub_level_accepted(SubLevel::Rf), Some(0.5));
        e.settings.rig_model = 3081; // IC-9700
        assert_eq!(e.sub_level_accepted(SubLevel::Rf), None);
        assert_eq!(e.sub_commandable(), None);
        assert_eq!(e.receivers().sub.expect("offered").rf_gain, None);
        assert!(
            taken(&mut e, true).is_empty(),
            "the other radio's request is not carried"
        );
    }

    /// ⚠️ THE RACE THE OWNER TOKEN CLOSES: the loop takes a write for one radio, the operator
    /// switches radio while the write is on the wire, and something touches the new radio's
    /// Sub state (re-keying it) before the old radio's answer arrives. That answer must land
    /// on nobody.
    #[test]
    fn an_answer_for_one_radio_never_lands_on_the_next() {
        let mut e = routed(3078);
        e.request_sub_level(SubLevel::Rf, 0.5).unwrap();
        let writes = e.take_sub_level_requests(true); // taken for the IC-7610
        e.settings.rig_model = 3081; // …the switch lands while it is on the wire
        let _ = e.request_sub_level(SubLevel::Rf, 0.25); // re-keys the state to the IC-9700
        e.observe_sub_level(writes[0], true); // the IC-7610's answer arrives
        assert_eq!(
            e.sub_level_accepted(SubLevel::Rf),
            None,
            "the IC-7610's RF is not the IC-9700's"
        );
    }

    /// The snapshot carries the route beside an offered Sub, and nothing for a radio with none.
    #[test]
    fn the_snapshot_says_whether_the_sub_can_be_commanded() {
        let mut e = station(3078, "general", "phone", 14.250, "20m", "USB");
        let rs = |e: &Engine| e.snapshot().radio.receivers.expect("receivers");
        assert_eq!(rs(&e).sub_commandable, None, "not reported yet");
        taken(&mut e, true);
        assert_eq!(rs(&e).sub_commandable, Some(true));
        taken(&mut e, false);
        assert_eq!(rs(&e).sub_commandable, Some(false));
        // A radio with no Sub offered says nothing about a route, whatever the loop reported.
        let mut one = station(3073, "general", "phone", 14.250, "20m", "USB");
        taken(&mut one, true);
        assert_eq!(rs(&one).sub_commandable, None);
        assert!(rs(&one).sub.is_none());
    }
}
