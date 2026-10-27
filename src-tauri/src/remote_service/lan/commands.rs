//! The command lane at the shack (`super::channel`, step 5 of its ladder): one paired computer's
//! calls, run as the shack's own screen runs them, the feeds it watches and the events it is sent.
//! It opens only on a connection whose computer named this same build (`super::wire`).
//!
//! ## A call
//!
//! The reading task parses it and hands it on, and never runs a command or takes a lock itself.
//! - **A Stop**, or the off half of a transmit switch, is admitted there, with no lock
//!   (`command_dispatch::admit_stop`), and run at once, behind no other call, its answer ahead of
//!   everything waiting to be written. It needs no lease and takes no slot. It moves the transmit
//!   stand-down generation there and then, an atomic step, so every start read before it is void
//!   wherever it waits. An unkey (`command_dispatch::is_unkey`: the off half of a switch) moves
//!   the starts-voided generation instead, which voids them the same way and halts nothing. Each
//!   Stop command has one thread at work at most, never one per Stop,
//!   as the Stop road's stopper has: a Stop runs at once when none of its command is running,
//!   and Stops of that command that land while one runs are run once more after it, after all of
//!   them have landed, each answered from that run. A Stop of another command never waits behind it.
//!   Those with the same arguments share a run, and at most [`STOP_RUNS`] runs wait; a Stop past
//!   them is answered `busy`, its stand-down already taken, so a flood of Stops each with arguments
//!   of its own grows neither what waits nor what runs.
//! - **Any other call** is admitted off the reading task, since a store's revision can wait on the
//!   Engine: the dispatcher's lease, class, journal, caps and revisions, with the lease read live
//!   from the operations authority. It then runs through the app's own handler, and its answer goes
//!   in its turn. At most [`PENDING`] wait at once from one computer; one more is refused `busy`
//!   on the reading task, before any of that.
//!
//! What a call answers: its value (with the revision of the store it read or saved), its own error,
//! the dispatcher's refusal, or, for an id already used, what became of it, and it is not run again.
//!
//! ## A transmission started from this computer
//!
//! A start is admitted only under the stream's transmit rules, which the dispatcher reads off the
//! caller: the lease on this connection, and what this connection's Stop road says
//! (`super::stop_road::Ready`: its presence live, the state its computer's screen last fetched under
//! two seconds old). A connection with no Stop road starts nothing. Once a start has run, its road's
//! presence is read again: a start that ran after presence lapsed is stopped at once.
//!
//! ⛔ Stop wins over a start that was in flight. A start is bound, as the reading task reads it, to
//! the transmit stand-down generation and the starts-voided generation (`command_dispatch::Afar`),
//! written into its arguments; its handler runs it only if both still stand under the Engine lock
//! it runs in (`command_dispatch::still_bound`). A Stop read after it on this link, a Stop on its
//! road, presence lapsing, or any halt at the station moves the stand-down, and an unkey read after
//! it moves the starts-voided generation, so a start that waited behind one of them never runs, and
//! is answered `stopped`. The other computer cannot say it: anything it sends
//! under that argument goes before the call is looked at.
//!
//! A Stop pressed after a start can also reach the shack first: by the second road, which on the
//! internet is the shorter way, or ahead of the start on the link, whose outbox writes a Stop before
//! anything waiting. So every call carries how many Stops its computer had pressed when it was made,
//! a Stop counting itself, and each Stop is noted by that number where this connection's road notes
//! its own (`super::stop_road::Ready`), before it moves the generation. A start whose number is
//! older than a Stop noted there is refused `stopped` as it is read, and never runs. The number
//! refuses nothing else: a Stop is taken whatever it carries.
//!
//! ## Feeds and events
//!
//! A watched feed runs its command here at its own pace, through the same handler, and pushes what
//! it answers, latest only (`super::outbox`). An event goes on when the dispatcher says it crosses
//! (`forwards`). Both only while this computer holds station control, as every call but a Stop:
//! nobody else is shown the station (the operator's ruling of 2026-09-27, "Controller only, for
//! now").
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use tokio::sync::broadcast;

use super::super::operations::Authority;
use super::outbox::Outbox;
use super::stop_road::Ready;
use super::wire::{self, Assembly, Feed, Whole};
use crate::command_classes::{class_of, Class};
use crate::command_dispatch::{
    admit_stop, forwards, is_unkey, takes_afar, Admission, Afar, Answer, Call, Caller, CallerKey,
    Dispatcher, Outcome, Profile, Refusal, Reply, Runner, ShackStores, AFAR, STOPPED_SINCE,
};
use tempo_app::remote_control::Revocation;

/// Calls one computer may have waiting at once, admitted or not: the dispatcher's own cap on calls
/// in flight, and as many again being admitted. One more is refused `busy` without a thread.
pub(crate) const PENDING: usize = 2 * crate::command_dispatch::IN_FLIGHT;
/// The most ids one `outcomes` may ask after: the calls in flight when a link dropped (the wire's).
pub(crate) use super::wire::OUTCOMES;
/// The most feeds one `watch` may name.
pub(crate) const WATCH: usize = 8;
/// Events kept for a lane that has fallen behind; older ones are skipped.
const EVENTS: usize = 64;
/// Runs of one Stop command waiting at once, each with its own arguments. A Stop's arguments only
/// differ for a switch's off half, and the most off halves one command has is `js8_arm`'s five
/// switches, so this is room to spare. A Stop past it is refused `busy`: what it stops was already
/// stopped (the stand-down moves on the reading task), and a run of its command is waiting.
pub(crate) const STOP_RUNS: usize = 8;

/// An event raised at the shack, as the lane forwards it: its name, a number the shack gives it
/// (the same on every connection), when it was raised, and its payload as the app emitted it.
#[derive(Clone, Debug)]
pub(crate) struct Event {
    pub(crate) name: &'static str,
    pub(crate) id: u64,
    pub(crate) at_ms: u64,
    pub(crate) data: String,
}

/// What the lane takes from the app: how a call runs, the events raised, and the Engine's
/// starts-voided counter (`Engine::remote_starts_voided`), which an unkey moves.
#[derive(Clone)]
pub(crate) struct Commands {
    pub(crate) runner: Runner,
    pub(crate) events: broadcast::Sender<Event>,
    pub(crate) voided: Revocation,
}

impl Commands {
    /// The app's handler, the Engine's starts-voided counter and a fresh event channel: what the
    /// app hands the service.
    pub(crate) fn new(runner: Runner, voided: Revocation) -> Self {
        Self {
            runner,
            events: broadcast::channel(EVENTS).0,
            voided,
        }
    }

    /// An event the app raised, numbered and timed here, for every lane open now.
    pub(crate) fn raised(&self, name: &'static str, data: String) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let _ = self.events.send(Event {
            name,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            at_ms: super::super::now_ms(),
            data,
        });
    }
}

impl Default for Commands {
    /// No handler: every call is answered with an error, and no event is raised. A service built
    /// without the app (a test's) has nothing to run.
    fn default() -> Self {
        Self::new(
            Arc::new(|_: String, _: Value, answer: Answer| {
                answer(Err("This station runs no commands.".to_string()))
            }),
            Revocation::default(),
        )
    }
}

/// What every call, feed and event on one lane needs.
struct Context {
    dispatcher: Dispatcher<ShackStores>,
    runner: Runner,
    authority: Arc<Authority>,
    connection: u64,
    device: String,
    outbox: Outbox,
    /// What this connection's Stop road says of a start from its computer; `None` with no road.
    ready: Option<Arc<Ready>>,
    /// The transmit stand-down generation (the authority's, which every halt at the station moves
    /// too): what a start is bound to as it is read, and what a Stop moves as it is read.
    transmit: Revocation,
    /// The starts-voided generation (the Engine's): what a start is bound to as it is read too, and
    /// what an unkey moves as it is read, in place of the stand-down.
    voided: Revocation,
}

impl Context {
    /// Whether this computer holds station control now. Never asked for a Stop.
    fn controls(&self) -> bool {
        self.authority
            .lan_controls(self.connection, &self.device, Instant::now())
    }

    /// The Stop numbered `stops` came from this computer on the command link: noted where its road
    /// notes the road's own, before the generation moves. A connection with no road starts nothing.
    fn took_stop(&self, stops: u64) {
        if let Some(ready) = &self.ready {
            ready.took_stop(stops);
        }
    }

    /// Whether a start made when its computer had pressed `stops` Stops came before a Stop that
    /// either of this connection's roads has since delivered.
    fn pressed_before_a_stop(&self, stops: u64) -> bool {
        self.ready
            .as_ref()
            .is_some_and(|ready| ready.pressed_before_a_stop(stops))
    }

    /// The stream's transmit rules as this connection's roads read them now, for a start: a Stop
    /// road at all, then its presence and the state its computer's screen last fetched.
    fn roads(&self, now: Instant) -> Result<(), Refusal> {
        match &self.ready {
            None => Err(Refusal::NoStopRoad),
            Some(ready) => ready.start(now),
        }
    }

    /// ⛔ A START THAT RAN AFTER ITS COMPUTER'S PRESENCE LAPSED IS STOPPED AT ONCE. Presence is
    /// read before a start is admitted, and the start then waits its turn at the Engine: a lapse in
    /// between has already halted the station, so what the start began would be on the air with no
    /// presence left to halt it. Asked once the start's handler has answered, whether it ran or a
    /// Stop since it was read kept it from running; the Stop is the station's own.
    fn present_after(&self, cmd: &str) {
        let live = self
            .ready
            .as_ref()
            .is_some_and(|ready| ready.presence_live(Instant::now()));
        if !live {
            tempo_core::applog::warn(
                "remote",
                &format!(
                    "on this network: {cmd} ran after its computer's presence lapsed; stopped"
                ),
            );
            let _ = run(&self.runner, "halt_tx", json!({}));
        }
    }
}

/// The open lane on one connection.
pub(super) struct Lane {
    context: Arc<Context>,
    key: CallerKey,
    profile: Profile,
    pending: Arc<AtomicUsize>,
    /// Each Stop command's Stops, run in turn: a Stop is admitted only for a command of Stop's rules
    /// (`command_dispatch::admit_stop`), so these are a few at most.
    stops: std::collections::HashMap<String, Arc<InTurn<StopRun>>>,
    /// The `outcomes` questions, answered in turn.
    asked: Arc<InTurn<Vec<u64>>>,
    assembly: Assembly,
    feeds: Vec<(&'static Feed, tokio::task::JoinHandle<()>)>,
    events: tokio::task::JoinHandle<()>,
}

/// What opening the lane takes: the station's parts and this connection's.
pub(super) struct Opening {
    pub dispatcher: Dispatcher<ShackStores>,
    pub commands: Commands,
    pub authority: Arc<Authority>,
    pub connection: u64,
    pub device: String,
    pub run: u64,
    pub profile: Profile,
    pub outbox: Outbox,
    /// What this connection's Stop road says of a start; `None` when it has no road.
    pub ready: Option<Arc<Ready>>,
}

impl Lane {
    /// The lane open, and forwarding events.
    pub(super) fn open(opening: Opening) -> Self {
        let context = Arc::new(Context {
            dispatcher: opening.dispatcher,
            runner: opening.commands.runner,
            transmit: opening.authority.transmit_revocation(),
            voided: opening.commands.voided,
            authority: opening.authority,
            connection: opening.connection,
            device: opening.device.clone(),
            outbox: opening.outbox,
            ready: opening.ready,
        });
        let events = tokio::spawn(forward(
            context.clone(),
            opening.commands.events.subscribe(),
        ));
        Self {
            key: CallerKey {
                device: opening.device,
                run: opening.run,
            },
            profile: opening.profile,
            pending: Arc::new(AtomicUsize::new(0)),
            stops: Default::default(),
            asked: Default::default(),
            assembly: Assembly::new(wire::CALL_BYTES),
            feeds: Vec::new(),
            events,
            context,
        }
    }

    /// One piece of a call: the call's JSON once its last piece is here. `Err` for a piece no
    /// computer of this protocol sends, which ends the connection.
    pub(super) fn piece(&mut self, frame: &[u8]) -> Result<Option<String>, ()> {
        match self.assembly.piece(frame)? {
            None => Ok(None),
            Some(Whole::Text(text)) => Ok(Some(text)),
            Some(Whole::Binary(_)) => Err(()),
        }
    }

    /// One call, on the reading task: a Stop admitted here and run at once, anything else handed
    /// off to be admitted, a start bound as it is read.
    pub(super) fn call(&mut self, mut call: Call) {
        let id = call.id;
        let stops = call.stops;
        // Whether a call is from afar is the lane's to say, never the other computer's.
        if let Some(args) = call.args.as_object_mut() {
            args.remove(AFAR);
        }
        let call = match admit_stop(call) {
            Ok(stop) => {
                // ⛔ STOP WINS: noted by its number first, where a start read after it is judged;
                // then every start read before this Stop is void from now, an atomic step taken
                // before the Stop waits for anything, as the road's Stop takes it. ⛔ An unkey
                // voids them by its own generation and halts nothing: moving the stand-down would
                // end everything started from afar on the radio loop's next tick
                // (`Engine::poll_remote_transmit`), the TX latch included.
                self.context.took_stop(stops);
                if is_unkey(&stop.cmd, &stop.args) {
                    self.context.voided.revoke();
                } else {
                    self.context.transmit.revoke();
                }
                let (context, cmd) = (self.context.clone(), stop.cmd.clone());
                let turns = self.stops.entry(stop.cmd).or_default();
                let args = stop.args;
                let mut over = false;
                turns.start(
                    |waiting| match waiting.iter().position(|run| run.args == args) {
                        Some(same) => waiting[same].ids.push(id),
                        None if waiting.len() >= STOP_RUNS => over = true,
                        None => waiting.push(StopRun {
                            args,
                            ids: vec![id],
                        }),
                    },
                    move |waiting| {
                        for StopRun { args, ids } in waiting {
                            let ran = run(&context.runner, &cmd, args);
                            for id in ids {
                                let _ = context.outbox.urgent(answered(id, ran.clone(), None));
                            }
                        }
                    },
                );
                if over {
                    let _ = self.context.outbox.urgent(refused(id, Refusal::Busy));
                }
                return;
            }
            Err(call) => call,
        };
        if self.pending.load(Ordering::SeqCst) >= PENDING {
            let _ = self.context.outbox.send(refused(id, Refusal::Busy));
            return;
        }
        let waiting = Waiting::new(&self.pending);
        let (context, key, profile) = (self.context.clone(), self.key.clone(), self.profile);
        let starts = class_of(&call.cmd) == Some(Class::StartsTx);
        // Read here, on the reading task: a Stop read after this call moves it after this read.
        let afar = takes_afar(&call.cmd).then(|| Afar {
            stand_down: self.context.transmit.generation(),
            starts_voided: self.context.voided.generation(),
        });
        // ⛔ Then the Stops that overtook it: one pressed after this start that reached the shack
        // first, by the road or ahead of it on the link. Read after the generation, as each Stop is
        // noted before it moves the generation, so no Stop falls between the two reads.
        let overtaken = starts && self.context.pressed_before_a_stop(stops);
        tokio::task::spawn_blocking(move || {
            let _waiting = waiting;
            let caller = Caller {
                key,
                profile,
                holds_lease: context.controls(),
                roads: if overtaken {
                    Err(Refusal::Stopped)
                } else if starts {
                    context.roads(Instant::now())
                } else {
                    Ok(())
                },
            };
            let cmd = call.cmd.clone();
            match context.dispatcher.admit(&caller, call) {
                // `admit_stop` took every Stop; one here still runs at once, as a Stop.
                Admission::Stop { args } => {
                    let _ =
                        context
                            .outbox
                            .urgent(answered(id, run(&context.runner, &cmd, args), None));
                }
                Admission::Run { mut args, ticket } => {
                    if let Some(afar) = afar {
                        bind(&mut args, afar);
                    }
                    let ran = run(&context.runner, &cmd, args);
                    if starts {
                        context.present_after(&cmd);
                    }
                    let voided = starts
                        && matches!(&ran, Some(Err(error)) if error.as_str() == STOPPED_SINCE);
                    let answer = match ran {
                        // A Stop came after it was read: it did not run.
                        Some(_) if voided => {
                            ticket.refuse(Refusal::Stopped);
                            refused(id, Refusal::Stopped)
                        }
                        Some(result) => {
                            let Reply { result, revision } = ticket.finish(result);
                            answered(id, Some(result), revision.as_ref().map(|r| r.as_str()))
                        }
                        // Its answer never came: the ticket records it as lost.
                        None => {
                            drop(ticket);
                            repeated(id, &Outcome::Lost)
                        }
                    };
                    let _ = context.outbox.send(answer);
                }
                Admission::Answered(outcome) => {
                    let _ = context.outbox.send(repeated(id, &outcome));
                }
                Admission::Refused(refusal) => {
                    let _ = context.outbox.send(refused(id, refusal));
                }
            }
        });
    }

    /// What became of each of `ids`, asked after a reconnect, answered off the reading task, in
    /// turn with the questions before it.
    pub(super) fn outcomes(&self, ids: Vec<u64>) {
        let (context, key) = (self.context.clone(), self.key.clone());
        self.asked.start(
            |waiting| waiting.push(ids),
            move |waiting| {
                for ids in waiting {
                    let outcomes: Vec<Value> = context
                        .dispatcher
                        .outcomes(&key, &ids)
                        .iter()
                        .map(outcome)
                        .collect();
                    let answer = json!({"type":"outcomes","ids":ids,"outcomes":outcomes});
                    let _ = context.outbox.text(answer.to_string());
                }
            },
        );
    }

    /// The feeds this computer watches from now on, in place of the ones before: each new one
    /// starts, each dropped one stops (its numbers and its deflate stream carry on if it is watched
    /// again), and the computer is told which names are no feed of this build's.
    pub(super) fn watch(&mut self, names: Vec<String>) {
        let mut watching: Vec<&'static Feed> = Vec::new();
        let mut unknown = Vec::new();
        for name in names {
            match wire::feed(&name) {
                Some(feed) if !watching.iter().any(|f| f.name == feed.name) => watching.push(feed),
                Some(_) => {}
                None => unknown.push(name),
            }
        }
        self.feeds.retain(|(feed, task)| {
            let kept = watching.iter().any(|f| f.name == feed.name);
            if !kept {
                task.abort();
            }
            kept
        });
        for feed in &watching {
            if !self.feeds.iter().any(|(f, _)| f.name == feed.name) {
                self.feeds
                    .push((feed, tokio::spawn(push(self.context.clone(), feed))));
            }
        }
        let names: Vec<&str> = watching.iter().map(|feed| feed.name).collect();
        let answer = json!({"type":"watching","feeds":names,"unknown":unknown});
        let _ = self.context.outbox.text(answer.to_string());
    }
}

impl Drop for Lane {
    /// The connection ended: its feeds and its events stop. A call still running finishes, and its
    /// outcome is kept for a reconnect to ask after.
    fn drop(&mut self) {
        self.events.abort();
        for (_, task) in &self.feeds {
            task.abort();
        }
    }
}

/// The Stops of one command waiting to run with the same arguments, and their ids.
struct StopRun {
    args: Value,
    ids: Vec<u64>,
}

/// Work the reading task hands off that must never take a thread a message: what waits is done in
/// turn by one blocking task, started when work arrives with none at work, and ending when none is
/// left. A flood therefore holds one thread, and what it holds waiting is what the network brought.
pub(super) struct InTurn<T> {
    /// What waits, and whether a task is at work on it.
    waiting: std::sync::Mutex<(Vec<T>, bool)>,
}

impl<T> Default for InTurn<T> {
    fn default() -> Self {
        Self {
            waiting: std::sync::Mutex::new((Vec::new(), false)),
        }
    }
}

impl<T: Send + 'static> InTurn<T> {
    fn waiting(&self) -> std::sync::MutexGuard<'_, (Vec<T>, bool)> {
        self.waiting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `add` puts work with what waits; a task to do it, with `work`, starts unless one is at work,
    /// which then does it once what it has now is done.
    pub(super) fn start(
        self: &Arc<Self>,
        add: impl FnOnce(&mut Vec<T>),
        mut work: impl FnMut(Vec<T>) + Send + 'static,
    ) {
        {
            let mut waiting = self.waiting();
            add(&mut waiting.0);
            if std::mem::replace(&mut waiting.1, true) {
                return;
            }
        }
        let turns = self.clone();
        off_the_reading_task(move || {
            let _at_work = AtWork(&turns);
            loop {
                let next = {
                    let mut waiting = turns.waiting();
                    if waiting.0.is_empty() {
                        // Under the same lock `start` reads it by: work added after this starts
                        // a task of its own.
                        waiting.1 = false;
                        return;
                    }
                    std::mem::take(&mut waiting.0)
                };
                work(next);
            }
        });
    }
}

/// ⛔ A task at work that panics is no longer at work: the next work starts a task of its own, so
/// a Stop after it is never left waiting for a task that is gone.
struct AtWork<'a, T: Send + 'static>(&'a InTurn<T>);

impl<T: Send + 'static> Drop for AtWork<'_, T> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.waiting().1 = false;
        }
    }
}

/// A Stop's or an `outcomes`' task, started off the reading task on a blocking thread.
fn off_the_reading_task(work: impl FnOnce() + Send + 'static) {
    #[cfg(test)]
    STARTED.with(|started| started.set(started.get() + 1));
    tokio::task::spawn_blocking(work);
}

#[cfg(test)]
thread_local! {
    /// The tasks [`off_the_reading_task`] started from this thread: a test's reading task runs on
    /// the test's own thread, so its count is the test's alone.
    pub(super) static STARTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A call waiting, counted against [`PENDING`] until it is answered however it ends.
struct Waiting(Arc<AtomicUsize>);

impl Waiting {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count.clone())
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A start's arguments with the stand-down it was read under, as its handler takes them
/// (`command_dispatch::still_bound`). An admitted call's arguments are always an object.
fn bind(args: &mut Value, afar: Afar) {
    if let (Some(fields), Ok(afar)) = (args.as_object_mut(), serde_json::to_value(afar)) {
        fields.insert(AFAR.to_string(), afar);
    }
}

/// What the app's own handler answers `cmd` with `args`, on this thread, or `None` if it gave no
/// answer at all (it dropped it, or the command panicked).
fn run(runner: &Runner, cmd: &str, args: Value) -> Option<Result<Value, String>> {
    let (answer, answered) = std::sync::mpsc::sync_channel(1);
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner(
            cmd.to_string(),
            args,
            Box::new(move |result| {
                let _ = answer.send(result);
            }),
        )
    }));
    ran.ok()?;
    answered.recv().ok()
}

/// A call's answer: its value or its error, with the revision of the store it read or saved; a
/// call whose answer never came is answered as lost. An answer over [`wire::ANSWER_BYTES`] goes as
/// that error instead.
fn answered(id: u64, result: Option<Result<Value, String>>, revision: Option<&str>) -> Whole {
    let mut answer = match result {
        Some(Ok(value)) => json!({"type":"result","id":id,"value":value}),
        Some(Err(error)) => json!({"type":"result","id":id,"error":error}),
        None => return repeated(id, &Outcome::Lost),
    };
    if let Some(revision) = revision {
        answer["revision"] = json!(revision);
    }
    let text = answer.to_string();
    if text.len() > wire::ANSWER_BYTES {
        let error = json!({"type":"result","id":id,"error":wire::ANSWER_TOO_LARGE});
        return Whole::Text(error.to_string());
    }
    Whole::Text(text)
}

/// A refused call: nothing ran.
fn refused(id: u64, refusal: Refusal) -> Whole {
    Whole::Text(json!({"type":"result","id":id,"refused":refusal.code()}).to_string())
}

/// A call whose id was used before: what became of it, and it did not run again.
fn repeated(id: u64, outcome_of: &Outcome) -> Whole {
    let mut answer = outcome(outcome_of);
    answer["type"] = json!("result");
    answer["id"] = json!(id);
    Whole::Text(answer.to_string())
}

/// An outcome as the wire says it.
fn outcome(outcome: &Outcome) -> Value {
    match outcome {
        Outcome::Running => json!({"outcome":"running"}),
        Outcome::Done => json!({"outcome":"done"}),
        Outcome::Failed(error) => json!({"outcome":"failed","error":error}),
        Outcome::Refused(refusal) => json!({"outcome":"refused","refused":refusal.code()}),
        Outcome::Lost => json!({"outcome":"lost"}),
        Outcome::Unknown => json!({"outcome":"unknown"}),
    }
}

/// One watched feed: its command run at its pace while this computer holds control, and each
/// value pushed (every one, or each change).
async fn push(context: Arc<Context>, feed: &'static Feed) {
    let mut tick = tokio::time::interval(feed.every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last: Option<Vec<u8>> = None;
    loop {
        tick.tick().await;
        // When this value's state began to be read: a value read slowly is the older for it.
        let taken = Instant::now();
        let ran = {
            let context = context.clone();
            tokio::task::spawn_blocking(move || {
                if !context.controls() {
                    return None;
                }
                run(&context.runner, feed.command, json!({}))
            })
            .await
        };
        let Ok(Some(Ok(value))) = ran else {
            continue;
        };
        let Ok(json) = serde_json::to_vec(&value) else {
            continue;
        };
        if feed.always || last.as_deref() != Some(json.as_slice()) {
            last = Some(json.clone());
            context.outbox.feed(feed.name, json, taken);
        }
    }
}

/// Events, on to this computer: each one that crosses, while it holds control.
async fn forward(context: Arc<Context>, mut events: broadcast::Receiver<Event>) {
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        if !forwards(event.name) {
            continue;
        }
        let asked = context.clone();
        if !tokio::task::spawn_blocking(move || asked.controls())
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            continue;
        };
        let message = json!({"type":"event","name":event.name,"id":event.id,"at":event.at_ms,
            "data":data});
        let _ = context.outbox.text(message.to_string());
    }
}
