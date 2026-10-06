//! ⛔ A CLOCK REPAIR NEVER PAUSES A MESSAGE (the operator's ruling, 2026-10-06: "Refuse the press
//! mid-message"). [`Engine::message_in_progress`] is up from a message's first over to its last,
//! for each way Nexus splits one across overs, and down before the message starts and once it has
//! gone. `tempo_audio`'s Repair clock press is refused while it is up; these drive the engine's
//! own queues, reading it in the slot between two overs, where nothing is on the air.

use super::*;
use crate::engine::tests::dec_snr;

/// A Tempo station (TempoFast, chat) with its identity set.
fn tempo() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::TempoFast);
    e
}

/// The first slot of the station's own T/R cycle.
fn own_slot(e: &Engine) -> u64 {
    u64::from(!e.tx_even())
}

/// Send what is queued, one over per own slot from `from`, and read the answer in the slot after
/// each over, the one between two overs. One reading per over.
fn in_progress_after_each_over(e: &mut Engine, from: u64) -> Vec<bool> {
    let mut read = Vec::new();
    let mut slot = from;
    while e.plan_tx(slot).is_some() {
        assert!(e.plan_tx(slot + 1).is_none(), "slot {}: not ours", slot + 1);
        read.push(e.message_in_progress());
        slot += 2;
    }
    read
}

/// Up after every over but the last, once per over.
fn up_until_the_last(overs: usize) -> Vec<bool> {
    (1..=overs).map(|n| n < overs).collect()
}

/// A broadcast's chunks: up from the first to the last, and down while it only waits.
#[test]
fn a_broadcast_is_in_progress_from_its_first_over_to_its_last() {
    let mut e = tempo();
    e.broadcast("THIS ONE TAKES SEVERAL OVERS TO SAY");
    let overs = e.broadcast_queue.len();
    assert!(overs > 2, "premise: several overs: {:?}", e.broadcast_queue);
    assert!(
        !e.message_in_progress(),
        "queued, not started: the hold makes it wait whole"
    );
    let from = own_slot(&e);
    assert_eq!(
        in_progress_after_each_over(&mut e, from),
        up_until_the_last(overs)
    );
}

/// A directed message goes out as its identify frame and then its chunks: up from the identify
/// frame to the last chunk, and down while it is held for the recipient.
#[test]
fn a_directed_message_is_in_progress_from_its_identify_frame_to_its_last_chunk() {
    let mut e = tempo();
    e.send_message("W9XYZ", "THIS ONE TAKES SEVERAL OVERS TO SAY");
    let from = own_slot(&e);
    assert!(
        e.plan_tx(from).is_none() && !e.message_in_progress(),
        "premise: held, W9XYZ not heard yet"
    );
    // W9XYZ is heard, so store-and-forward releases the message.
    e.ingest_decodes_for_test(&[dec_snr("CQ W9XYZ EN37", -8)], from + 1);
    let read = in_progress_after_each_over(&mut e, from + 2);
    assert!(
        read.len() > 2,
        "premise: its identify frame and chunks: {read:?}"
    );
    assert_eq!(read, up_until_the_last(read.len()));
}

/// A QSY directive is queued AHEAD of whatever waits, so the next over can be the directive's own
/// first chunk while a broadcast behind it is still part-way through. That broadcast counts. A
/// directive queued alone has not started (the control).
#[test]
fn a_qsy_directive_queued_ahead_does_not_hide_a_broadcast_part_way_through() {
    let mut e = tempo();
    e.broadcast("THIS ONE TAKES SEVERAL OVERS TO SAY");
    assert!(e.plan_tx(own_slot(&e)).is_some(), "premise: its first over");
    e.enqueue_qsy_directive(&Directive::Home);
    assert_eq!(
        tempo_core::text::parse_chunk(&e.broadcast_queue[0]).map(|(_, seq, _, _)| seq),
        Some(1),
        "premise: the directive's first chunk goes next: {:?}",
        e.broadcast_queue
    );
    assert!(e.message_in_progress());

    let mut alone = tempo();
    alone.enqueue_qsy_directive(&Directive::Home);
    assert!(!alone.message_in_progress(), "control: a directive alone");
}

/// A JS8 message's frames, one per period: up from the first to the last.
#[test]
fn a_js8_message_is_in_progress_from_its_first_frame_to_its_last() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.js8_enter();
    e.set_frequency(14.078, "20m", "USB");
    e.set_tx_enabled(true);
    e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
        .expect("queues");
    let frames = e.js8_state().queue.len();
    assert!(frames > 1, "premise: several frames: {frames}");
    assert!(!e.message_in_progress(), "queued, not started");
    let mut read = Vec::new();
    let mut slot = 0;
    while e.plan_tx(slot).is_some() {
        read.push(e.message_in_progress());
        slot += 1;
    }
    assert_eq!(read, up_until_the_last(frames));
}

/// A message that fits one over is never in progress: nothing of it is left to pause.
#[test]
fn a_message_of_one_over_is_never_in_progress() {
    let mut e = tempo();
    e.broadcast("73");
    let from = own_slot(&e);
    assert_eq!(in_progress_after_each_over(&mut e, from), vec![false]);
}
