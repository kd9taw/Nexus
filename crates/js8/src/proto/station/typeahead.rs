//! Typeahead — JS8Call's compose box while its message goes out (`prepareNextMessageFrame`,
//! JS8_UI/mainwindow.cpp:3589-3668, read as the spec; its user guide: "you can start transmitting
//! and continue typing your message as each frame is transmitted. Checksummed messages like MSG or
//! Relays cannot use typeahead.").
//!
//! JS8Call's box holds the message as its frames render, the characters of each frame marked sent
//! as it goes (`setCharsSent`, :3653-3654). Before each frame (`stopTx` → `prepareNextMessageFrame`,
//! :2895) it reads the box: changed and not empty, the frames still to go are thrown away and the
//! unsent text is framed again (:3600-3628):
//! - nothing of the message gone yet: the box's text is the message;
//! - part way out: the rest as data only (`forceData`, Varicode.cpp:2055-2058): no heartbeat,
//!   compound or directed frame, no selected call put in front, no "MYCALL: ", none flagged first and
//!   the final one flagged last (mainwindow.cpp:3636-3643);
//! - its last frame gone (it carried the last flag): a message of its own, sent straight after, its
//!   first frame not flagged first (:3636-3638).
//!
//! A message whose directed command is checksummed (MSG, a relay; `isCommandChecksumed`, :3573-3576)
//! leaves the box read-only while it goes. Emptying what is still to go ends the message after the
//! frame on the air, with no last flag sent, as JS8Call's empty rebuild ends it; emptying a box with
//! nothing of it gone changes nothing (`!isEmpty()`, :3602).
//!
//! Here the cockpit's box hands the station what is still to go ([`Station::type_ahead`]) and the
//! station frames the newest text just before the next frame goes ([`Station::take_typeahead`],
//! from `next_frame`), as JS8Call reads its box then. [`Station::typeahead`] reads back what has gone
//! and what is still to go, for the box to show. One box, one message: the operator's message the box
//! sent ([`Station::begin_typeahead`]), marked in the outbox (`OutMsg::typeahead`).

use super::{FreqHint, Origin, OutMsg, Station, MAX_OUTBOX};
use crate::phy::I3;
use crate::proto::callsign::CallRef;
use crate::proto::compose::{
    continuation_frames, frames_with_grid, max_frames, normalise, ComposeError,
};
use crate::proto::frame::Frame;

/// The operator's message the compose box is typing into, while it goes out.
#[derive(Debug, Clone)]
pub(super) struct Typeahead {
    /// The addressee it was sent to (JS8Call's selected call), for a message framed again whole:
    /// before its first frame goes, or as a follow-on after its last.
    to: Option<CallRef>,
    /// What the frames handed out carried, as they render (JS8Call's `m_totalTxMessage`).
    sent: String,
    /// What the frames still to go carry, as they render.
    unsent: String,
    /// The box's newest text for what is still to go, not yet framed (JS8Call's dirty box).
    edit: Option<String>,
    /// A checksummed directed command (MSG, a relay): the box may not change it.
    locked: bool,
    /// The message in the outbox went in after an earlier one's last frame: its first frame is not
    /// flagged first, as JS8Call clears the flag once a frame has gone (mainwindow.cpp:3636-3638).
    follow_on: bool,
    /// The period start its last frame was handed out in, once it has been.
    last_out_ms: Option<u64>,
}

/// What the compose box shows of the message it is typing into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeaheadView {
    /// What has gone, as the frames render.
    pub sent: String,
    /// What is still to go: the box's newest text once given, the frames' until then.
    pub unsent: String,
    /// MSG or a relay: the box may not change it.
    pub locked: bool,
}

/// Why the box's text was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeaheadError {
    /// None of the operator's messages is going out: it has gone, or it was stopped.
    NotSending,
    /// MSG or a relay, which JS8Call's box keeps read-only while it goes.
    Locked,
    /// The box's text does not start with what has gone since it last looked: a change to text
    /// already sent.
    Stale,
    /// The text would not frame: over the airtime cap.
    Compose(ComposeError),
}

/// How the box's newest text is framed (JS8Call's three cases).
enum Retype {
    /// Nothing to do: an empty box with nothing of the message gone, or after its last frame.
    Keep,
    /// The whole message again, nothing of it gone yet.
    Whole(Vec<(Frame, I3)>),
    /// The frames still to go, as data. None ends the message after the frame on the air.
    Rest(Vec<(Frame, I3)>),
    /// A message of its own after the last frame went, sent straight after it.
    FollowOn(Vec<(Frame, I3)>),
}

fn rendered(frames: &[(Frame, I3)]) -> String {
    frames.iter().map(|(f, _)| f.render()).collect()
}

/// JS8Call keeps the box read-only for a checksummed directed command (`isCommandChecksumed`,
/// mainwindow.cpp:3573-3576): MSG, MSG TO:, a relay, QUERY, QUERY MSGS, QUERY CALL, CMD.
fn checksummed(frames: &[(Frame, I3)]) -> bool {
    frames.iter().any(|(f, _)| match f {
        Frame::Directed { cmd, .. } | Frame::CompoundDirected { cmd, .. } => cmd.is_checksummed(),
        _ => false,
    })
}

/// The box's message handed out one more frame (`next_frame`): what has gone grows by it, and its
/// last frame starts the period the box lets go after.
pub(super) fn handed_out(
    session: &mut Option<Typeahead>,
    head: &OutMsg,
    frame: &Frame,
    period_start_ms: u64,
) {
    let Some(t) = session.as_mut().filter(|_| head.typeahead) else {
        return;
    };
    t.sent.push_str(&frame.render());
    t.unsent = rendered(&head.frames[head.cursor..]);
    if head.cursor >= head.frames.len() {
        t.last_out_ms = Some(period_start_ms);
    }
}

impl Station {
    /// The compose box keeps the operator's message it has just queued while it goes out, sent to
    /// `to`: the outbox's newest message. Not while another message of the box's is going out: the
    /// box holds one.
    pub fn begin_typeahead(&mut self, to: Option<&CallRef>) {
        if self.typeahead_live() {
            return;
        }
        for o in self.outbox.iter_mut() {
            o.typeahead = false;
        }
        let Some(o) = self
            .outbox
            .back_mut()
            .filter(|o| o.origin == Origin::Operator && o.cursor == 0)
        else {
            self.typeahead = None;
            return;
        };
        o.typeahead = true;
        self.typeahead = Some(Typeahead {
            to: to.cloned(),
            sent: String::new(),
            unsent: rendered(&o.frames),
            edit: None,
            locked: checksummed(&o.frames),
            follow_on: false,
            last_out_ms: None,
        });
    }

    /// What the compose box shows of the message it is typing into, while that goes out: until the
    /// period after its last frame, or until it is stopped.
    pub fn typeahead(&self) -> Option<TypeaheadView> {
        let t = self.typeahead.as_ref().filter(|_| self.typeahead_live())?;
        Some(TypeaheadView {
            sent: t.sent.clone(),
            unsent: t.edit.clone().unwrap_or_else(|| t.unsent.clone()),
            locked: t.locked,
        })
    }

    /// The box's text for what is still to go of the message going out. `sent_seen` is how many
    /// characters of it the box had seen go: whatever has gone since must start `unsent`, and is
    /// taken off. Framed just before the next frame ([`Self::take_typeahead`]), as JS8Call reads its
    /// box then; checked now, so a text over the airtime cap is refused as it is typed.
    pub fn type_ahead(&mut self, sent_seen: usize, unsent: &str) -> Result<(), TypeaheadError> {
        if !self.typeahead_live() {
            self.typeahead = None;
            return Err(TypeaheadError::NotSending);
        }
        let Some(t) = self.typeahead.as_ref() else {
            return Err(TypeaheadError::NotSending);
        };
        if t.locked {
            return Err(TypeaheadError::Locked);
        }
        let sent: Vec<char> = t.sent.chars().collect();
        let since: String = sent
            .get(sent_seen..)
            .ok_or(TypeaheadError::Stale)?
            .iter()
            .collect();
        let text = normalise(unsent);
        let rest = text
            .strip_prefix(since.as_str())
            .ok_or(TypeaheadError::Stale)?
            .to_string();
        self.retype(&rest).map_err(TypeaheadError::Compose)?;
        if let Some(t) = self.typeahead.as_mut() {
            t.edit = Some(rest);
        }
        Ok(())
    }

    /// Just before each frame: frame the box's newest text, and let the box go once the period
    /// after its message's last frame has come with nothing more typed (JS8Call clears its box as
    /// the message ends, mainwindow.cpp:2895-2899).
    pub(super) fn take_typeahead(&mut self, period_start_ms: u64) {
        if self.typeahead.is_none() {
            return;
        }
        if !self.typeahead_live() {
            self.typeahead = None;
            return;
        }
        if let Some(text) = self.typeahead.as_mut().and_then(|t| t.edit.take()) {
            // Checked when it was typed; what changed since (the speed, say) can refuse it, and the
            // frames already framed go as they are.
            if let Ok(r) = self.retype(&text) {
                self.apply_retype(r);
            }
        }
        if self
            .typeahead
            .as_ref()
            .and_then(|t| t.last_out_ms)
            .is_some_and(|at| period_start_ms > at)
        {
            self.typeahead = None;
        }
    }

    /// The 1 Hz tick's half: a box whose message's last frame went two periods ago lets go even
    /// when no frame is asked for (TX off), so it never shows a message that has gone.
    pub(super) fn expire_typeahead(&mut self, now_ms: u64) {
        let period_ms = u64::from(self.cfg.speed.period_s()) * 1000;
        if self
            .typeahead
            .as_ref()
            .and_then(|t| t.last_out_ms)
            .is_some_and(|at| now_ms >= at + 2 * period_ms)
        {
            self.typeahead = None;
        }
    }

    /// Whether frames follow the box's message's last frame: the box has more typed since that
    /// frame went, which goes out straight after it (Hold PTT between frames holds across it, as
    /// JS8Call's does once its box is framed again, mainwindow.cpp:2928).
    pub(super) fn typeahead_follows(&self) -> bool {
        self.typeahead.as_ref().is_some_and(|t| {
            t.last_out_ms.is_some() && t.edit.as_deref().is_some_and(|e| !e.trim().is_empty())
        })
    }

    /// Whether a message of the box's is going out: it is in the outbox, or its last frame has
    /// gone and the period after it has not come yet.
    fn typeahead_live(&self) -> bool {
        self.typeahead
            .as_ref()
            .is_some_and(|t| t.last_out_ms.is_some() || self.outbox.iter().any(|o| o.typeahead))
    }

    /// How the box's newest text is framed now.
    fn retype(&self, text: &str) -> Result<Retype, ComposeError> {
        let Some(t) = self.typeahead.as_ref() else {
            return Ok(Retype::Keep);
        };
        let speed = self.cfg.speed;
        // Every macro the station can value, none pruned, as a send replaces them (JS8Call frames
        // the box again through `appendMessage`, mainwindow.cpp:3443-3447); idle 0M, as the box was
        // just typed in.
        let text = crate::proto::macros::replace(text, &self.macro_values(0), false);
        let whole = |follow_on: bool| -> Result<Vec<(Frame, I3)>, ComposeError> {
            let mut frames = frames_with_grid(
                &self.cfg.mycall,
                &self.cfg.grid,
                t.to.as_ref(),
                &text,
                speed,
            )?;
            if let Some(first) = frames.first_mut().filter(|_| follow_on) {
                first.1.first = false;
            }
            Ok(frames)
        };
        Ok(match self.outbox.iter().find(|o| o.typeahead) {
            Some(o) if o.cursor == 0 => {
                if text.trim().is_empty() {
                    Retype::Keep
                } else {
                    Retype::Whole(whole(t.follow_on)?)
                }
            }
            Some(o) => {
                let rest = continuation_frames(&self.cfg.mycall, &text, speed);
                let (frames, max) = (o.cursor + rest.len(), max_frames(speed));
                if frames > max {
                    return Err(ComposeError::TooLong { frames, max });
                }
                Retype::Rest(rest)
            }
            None if text.trim().is_empty() => Retype::Keep,
            None => Retype::FollowOn(whole(true)?),
        })
    }

    /// Put the box's newest text, framed, in the outbox.
    fn apply_retype(&mut self, r: Retype) {
        let frames = match r {
            Retype::Keep => return,
            Retype::Whole(frames) | Retype::Rest(frames) => frames,
            Retype::FollowOn(frames) => {
                if self.outbox.len() >= MAX_OUTBOX {
                    return;
                }
                let unsent = rendered(&frames);
                let locked = checksummed(&frames);
                self.outbox.push_front(OutMsg {
                    origin: Origin::Operator,
                    display: unsent.trim_end().to_string(),
                    frames,
                    cursor: 0,
                    freq_hint: FreqHint::Dial,
                    typeahead: true,
                    mycall: self.cfg.mycall.clone(),
                });
                if let Some(t) = self.typeahead.as_mut() {
                    t.unsent = unsent;
                    t.locked = locked;
                    t.follow_on = true;
                    t.last_out_ms = None;
                }
                return;
            }
        };
        let Some(i) = self.outbox.iter().position(|o| o.typeahead) else {
            return;
        };
        let o = &mut self.outbox[i];
        o.frames.truncate(o.cursor);
        o.frames.extend(frames);
        if o.cursor >= o.frames.len() {
            // Nothing more to send: the message ends with the frame on the air, its last flag never
            // sent, as JS8Call's empty rebuild ends it.
            self.outbox.remove(i);
            self.typeahead = None;
            return;
        }
        o.display = rendered(&o.frames).trim_end().to_string();
        let (unsent, locked) = (rendered(&o.frames[o.cursor..]), checksummed(&o.frames));
        if let Some(t) = self.typeahead.as_mut() {
            t.unsent = unsent;
            t.locked = locked;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::Speed;
    use crate::proto::frame::decode_word;
    use crate::proto::station::StationConfig;

    /// Normal's period, ms.
    const P: u64 = 15_000;
    const LONG: &str = "TEST MESSAGE WITH MULTIPLE FRAMES, LONG ENOUGH TO GO ON FOR A WHILE";

    fn station() -> Station {
        Station::new(StationConfig {
            mycall: "KD9TAW".into(),
            grid: "EN52".into(),
            speed: Speed::Normal,
            ..StationConfig::default()
        })
    }

    /// The box's message, queued as the cockpit's Send queues it; how many frames it is.
    fn send(s: &mut Station, to: Option<&CallRef>, text: &str) -> usize {
        let n = s.send(to, text, 0).expect("queues");
        s.begin_typeahead(to);
        n
    }

    /// The frame keyed in the period starting at `at`, as a receiver decodes it: its text, first
    /// and last flags.
    fn key(s: &mut Station, at: u64) -> Option<(String, bool, bool)> {
        let mut seed = 7u32;
        let mut rng = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed
        };
        let tf = s.next_frame(at, &|_| false, &mut rng)?;
        let (frame, i3) = decode_word(&tf.word, Speed::Normal).expect("decodes");
        Some((frame.render(), i3.first, i3.last))
    }

    /// Every frame keyed from `at` on, a period apart, until none is.
    fn key_all(s: &mut Station, at: u64) -> Vec<(String, bool, bool)> {
        let mut out = Vec::new();
        let mut at = at;
        while let Some(f) = key(s, at) {
            out.push(f);
            at += P;
        }
        out
    }

    fn text(frames: &[(String, bool, bool)]) -> String {
        frames.iter().map(|f| f.0.as_str()).collect()
    }

    /// The flags a message's frames carry from the second on: none first, the final one last.
    fn rest_flags(frames: &[(String, bool, bool)]) -> bool {
        let (last, rest) = frames.split_last().expect("a frame");
        !last.1 && last.2 && rest.iter().all(|f| !f.1 && !f.2)
    }

    /// ⭐ PART WAY OUT, WHAT IS TYPED GOES OUT AS DATA IN THE FRAMES STILL TO GO. The first frame
    /// keys; the box's text then replaces what was still to go, and the frames after carry it, as
    /// data, none flagged first and the last one last, as JS8Call's `forceData` rebuild sends them.
    #[test]
    fn part_way_out_the_typed_text_goes_out_as_data_in_the_frames_still_to_go() {
        let mut s = station();
        assert!(send(&mut s, None, LONG) >= 3, "premise: several frames");
        let first = key(&mut s, 0).expect("the first frame");
        assert!(first.1 && !first.2, "premise: the message's first frame");
        let v = s.typeahead().expect("the box holds the message");
        assert_eq!(v.sent, first.0, "what has gone is the first frame's text");
        s.type_ahead(v.sent.chars().count(), "HELLO AGAIN")
            .expect("taken");
        let rest = key_all(&mut s, P);
        assert_eq!(text(&rest), "HELLO AGAIN");
        assert!(
            rest_flags(&rest),
            "data, none first, the last last: {rest:?}"
        );
    }

    /// ⭐ NOTHING GONE YET, THE BOX IS THE MESSAGE: it is framed again whole, with its addressee, as
    /// JS8Call frames its box at the first frame.
    #[test]
    fn before_its_first_frame_the_box_is_framed_again_as_the_whole_message() {
        let mut s = station();
        let to = CallRef::Base("W1AW".into());
        send(&mut s, Some(&to), "HELLO");
        let v = s.typeahead().expect("the box holds the message");
        assert_eq!(v.unsent, "KD9TAW: W1AW  HELLO", "as its frames render");
        s.type_ahead(0, "KD9TAW: W1AW  GOOD MORNING")
            .expect("taken");
        let all = key_all(&mut s, 0);
        assert_eq!(text(&all), "KD9TAW: W1AW  GOOD MORNING");
        assert!(all[0].1, "its first frame is first");
        assert!(rest_flags(&all[1..]));
    }

    /// ⭐ TYPED AFTER THE LAST FRAME WENT: A MESSAGE OF ITS OWN, STRAIGHT AFTER IT, ITS FIRST FRAME
    /// NOT FLAGGED FIRST (JS8Call clears the flag once a frame has gone). Hold PTT between frames
    /// holds across it. With nothing typed the period after, the box lets go.
    #[test]
    fn typed_after_the_last_frame_it_goes_straight_after_as_a_message_of_its_own() {
        let mut s = station();
        assert_eq!(send(&mut s, None, "HI"), 1, "premise: a one-frame message");
        let only = key(&mut s, 0).expect("its frame");
        assert!(only.1 && only.2, "premise: first and last");
        assert!(!s.frames_follow(), "nothing typed: nothing follows");
        let v = s
            .typeahead()
            .expect("the box still holds it while its frame is on the air");
        assert_eq!(v.unsent, "");
        s.type_ahead(v.sent.chars().count(), "MORE").expect("taken");
        assert!(
            s.frames_follow(),
            "the follow-on follows: Hold PTT holds across it"
        );
        let after = key_all(&mut s, P);
        assert_eq!(text(&after), "KD9TAW: MORE");
        assert!(
            rest_flags(&after),
            "not flagged first, the last last: {after:?}"
        );
        assert!(s.typeahead().is_none(), "the box let go the period after");
    }

    /// With nothing typed the box lets go at the period after the last frame, or at the 1 Hz tick
    /// two periods after it when no frame is asked for (TX off).
    #[test]
    fn the_box_lets_go_the_period_after_the_last_frame() {
        let mut s = station();
        send(&mut s, None, "HI");
        key(&mut s, 0).expect("its frame");
        assert!(
            s.typeahead().is_some(),
            "held while its frame is on the air"
        );
        assert!(
            key(&mut s, P).is_none() && s.typeahead().is_none(),
            "at the next frame"
        );
        let mut s = station();
        send(&mut s, None, "HI");
        key(&mut s, 0).expect("its frame");
        s.tick(2 * P - 1);
        assert!(s.typeahead().is_some(), "still within the period after");
        s.tick(2 * P);
        assert!(
            s.typeahead().is_none(),
            "at the tick, with no frame asked for"
        );
    }

    /// Emptying what is still to go ends the message after the frame on the air: nothing more
    /// keys, and its last flag is never sent, as JS8Call's empty rebuild ends it.
    #[test]
    fn emptying_what_is_still_to_go_ends_the_message_after_the_frame_on_the_air() {
        let mut s = station();
        send(&mut s, None, LONG);
        let first = key(&mut s, 0).expect("the first frame");
        assert!(!first.2, "premise: not the last");
        s.type_ahead(first.0.chars().count(), "").expect("taken");
        assert!(s.frames_follow(), "until the next frame is asked for");
        assert_eq!(key(&mut s, P), None, "nothing more keys");
        assert!(s.queue().is_empty() && s.typeahead().is_none());
    }

    /// Emptying the box before anything went changes nothing (`!isEmpty()`): the message goes as
    /// it was.
    #[test]
    fn emptying_the_box_before_anything_went_changes_nothing() {
        let mut s = station();
        let mut control = station();
        send(&mut s, None, LONG);
        send(&mut control, None, LONG);
        s.type_ahead(0, "").expect("taken");
        assert_eq!(key_all(&mut s, 0), key_all(&mut control, 0));
    }

    /// ⭐ A MSG IS READ-ONLY WHILE IT GOES, AS IN JS8CALL: a checksummed command's message takes no
    /// typing, and goes out as it was.
    #[test]
    fn a_msg_takes_no_typeahead() {
        let mut s = station();
        let mut control = station();
        let to = CallRef::Base("W1AW".into());
        send(&mut s, Some(&to), "MSG HELLO THERE");
        send(&mut control, Some(&to), "MSG HELLO THERE");
        assert!(s.typeahead().expect("the box holds it").locked);
        assert_eq!(
            s.type_ahead(0, "KD9TAW: W1AW MSG GOODBYE"),
            Err(TypeaheadError::Locked)
        );
        assert_eq!(key_all(&mut s, 0), key_all(&mut control, 0));
    }

    /// What went since the box last looked is taken off the front of its text; a change to text
    /// already sent is refused, and what is still to go stays as it was.
    #[test]
    fn what_went_since_the_box_looked_comes_off_the_front_and_a_change_to_it_is_stale() {
        let mut s = station();
        send(&mut s, None, LONG);
        let before = s.typeahead().expect("the box holds it");
        let first = key(&mut s, 0).expect("the first frame");
        // The box still shows the whole message (it has not looked since), plus what was typed.
        s.type_ahead(0, &format!("{} TOO", before.unsent))
            .expect("taken, what went taken off");
        let rest = key_all(&mut s, P);
        assert_eq!(
            format!("{}{}", first.0, text(&rest)),
            format!("{} TOO", before.unsent)
        );
        let mut s = station();
        send(&mut s, None, LONG);
        let v = s.typeahead().expect("the box holds it");
        key(&mut s, 0).expect("the first frame");
        assert_eq!(
            s.type_ahead(0, &format!("XX{}", &v.unsent[2..])),
            Err(TypeaheadError::Stale)
        );
    }

    /// Over the airtime cap, typing is refused as it is typed, and the frames go as they were.
    #[test]
    fn over_the_airtime_cap_typing_is_refused() {
        let mut s = station();
        let mut control = station();
        send(&mut s, None, LONG);
        send(&mut control, None, LONG);
        let first = key(&mut s, 0).expect("the first frame");
        key(&mut control, 0).expect("the control's first frame");
        assert!(matches!(
            s.type_ahead(first.0.chars().count(), &"X".repeat(2_000)),
            Err(TypeaheadError::Compose(ComposeError::TooLong { .. }))
        ));
        assert_eq!(
            key_all(&mut s, P),
            key_all(&mut control, P),
            "the frames go as they were"
        );
    }

    /// Typed text takes the grid macros as a send does.
    #[test]
    fn typed_text_takes_the_grid_macros() {
        let mut s = station();
        send(&mut s, None, LONG);
        let first = key(&mut s, 0).expect("the first frame");
        s.type_ahead(first.0.chars().count(), "QTH <MYGRID4>")
            .expect("taken");
        assert_eq!(text(&key_all(&mut s, P)), "QTH EN52");
    }

    /// Typed text takes every macro the station can value, as a send does: JS8Call frames the box
    /// again through `appendMessage`, which replaces them and prunes nothing (mainwindow.cpp:
    /// 3443-3447). A word in angle brackets that is no macro stays as typed.
    #[test]
    fn typed_text_takes_every_macro_a_send_takes() {
        let mut s = Station::new(StationConfig {
            info: "RIG IC7300".into(),
            ..station().cfg
        });
        send(&mut s, None, LONG);
        let first = key(&mut s, 0).expect("the first frame");
        s.type_ahead(
            first.0.chars().count(),
            "DE <MYCALL> <MYINFO> <MYREPLY> <MYVERSION> <ANT>",
        )
        .expect("taken");
        assert_eq!(
            text(&key_all(&mut s, P)),
            "DE KD9TAW RIG IC7300 HW CPY? NEXUS <ANT>"
        );
    }

    /// Stopped (Stop TX), the box lets go and takes nothing more.
    #[test]
    fn a_stopped_message_lets_the_box_go() {
        let mut s = station();
        send(&mut s, None, LONG);
        key(&mut s, 0).expect("the first frame");
        s.halt();
        assert!(s.typeahead().is_none());
        assert_eq!(s.type_ahead(0, "MORE"), Err(TypeaheadError::NotSending));
        assert_eq!(key(&mut s, P), None);
        // …and once its last frame has gone, Stop TX, TX off (the outbox dropped) and Drop queue
        // let it go too: nothing typed after them goes out as a follow-on.
        for stop in ["halt", "drop_outbox", "drop_queue"] {
            let mut s = station();
            send(&mut s, None, "HI");
            key(&mut s, 0).expect("its only frame");
            assert!(
                s.typeahead().is_some(),
                "{stop}: premise: held while it is on the air"
            );
            match stop {
                "halt" => s.halt(),
                "drop_outbox" => {
                    s.drop_outbox();
                }
                _ => s.drop_queue(),
            }
            assert!(s.typeahead().is_none(), "{stop}");
            assert_eq!(
                s.type_ahead(1, "MORE"),
                Err(TypeaheadError::NotSending),
                "{stop}"
            );
            assert_eq!(key(&mut s, P), None, "{stop}: nothing keys");
        }
    }

    /// Only the box's message counts: a message ahead of it in the queue (a CQ the operator
    /// clicked) goes out without touching the box, which then holds its own message.
    #[test]
    fn only_the_box_message_counts_as_sent() {
        let mut s = station();
        s.call_cq(0).expect("a CQ queued");
        send(&mut s, None, LONG);
        let cq = key(&mut s, 0).expect("the CQ");
        assert!(cq.0.contains("CQ"), "premise: the CQ goes first");
        let v = s.typeahead().expect("the box holds its message");
        assert_eq!(v.sent, "", "nothing of the box's message has gone");
    }
}
