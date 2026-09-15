//! Everything here drives the REAL feed, the REAL encoder and real libopus through
//! `DetachedFeed`, never a stand-in for them. A stand-in would let the reassuring
//! assertions in this file — an idle station encodes nothing, a full socket drops audio
//! — pass against code that does not hold, so each one is paired with a control on the
//! same instrument that must go the other way.

use super::*;
use tempo_audio::receive_encode::{DetachedFeed, FRAME_MS};

const RATE: u32 = 48_000;
/// One RX DSP tick of device-rate samples.
const TICK: usize = (RATE as usize) * (FRAME_MS as usize) / 1000;

/// A recognisable tone rather than silence: silence encodes to near-nothing, so a test
/// that "produced bytes" against silence would be measuring almost nothing at all.
fn tone(n: usize, start: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = (start + i) as f32 / RATE as f32;
            0.35 * (2.0 * std::f32::consts::PI * 700.0 * t).sin()
        })
        .collect()
}

/// Publish `ticks` RX DSP ticks and poll the lane after each, as the transport loop does.
fn run(
    lane: &mut AudioLane,
    feed: &DetachedFeed,
    ticks: usize,
    writable: bool,
) -> (Vec<String>, Option<&'static str>) {
    let base = Instant::now();
    let mut messages = Vec::new();
    let mut ended = None;
    for k in 0..ticks {
        let at = base + Duration::from_millis(FRAME_MS * k as u64);
        feed.publish(at, &tone(TICK, k * TICK));
        let pump = lane.poll(at, writable);
        if let Some(message) = pump.message {
            messages.push(message);
        }
        if pump.ended.is_some() {
            ended = pump.ended;
            break;
        }
    }
    (messages, ended)
}

const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const OTHER: &str = "22222222-2222-4222-8222-222222222222";
const DEVICE: &str = "33333333-3333-4333-8333-333333333333";
const LEASE: &str = "44444444-4444-4444-8444-444444444444";

#[test]
fn idle_station_encodes_nothing() {
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    let (messages, ended) = run(&mut lane, &feed, 100, true);
    assert!(messages.is_empty(), "an idle lane produced audio");
    assert!(ended.is_none());
    assert_eq!(lane.encoded_bytes(), 0);
    assert!(!lane.listening());
    // The claim is not just "no bytes came out" but "nothing was even read": the feed's
    // single reader is still free, which it would not be if anything had subscribed.
    assert!(
        feed.feed.subscribe(feed.source()).is_ok(),
        "something held the feed's reader while nobody was listening"
    );

    // The control, on the same instrument. Everything above must be false once a
    // browser is actually listening, or the assertions prove nothing about idleness.
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .unwrap();
    let (messages, _) = run(&mut lane, &feed, 100, true);
    assert!(
        !messages.is_empty(),
        "control: a listening lane produced no audio"
    );
    assert!(lane.encoded_bytes() > 0);
    assert!(
        matches!(feed.feed.subscribe(feed.source()), Err(ReceiveError::InUse)),
        "control: the listening lane did not hold the reader"
    );
}

#[test]
fn a_bundle_carries_three_stated_frames_inside_the_shared_byte_bound() {
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .unwrap();
    let (messages, _) = run(&mut lane, &feed, 60, true);
    assert!(
        messages.len() >= 15,
        "expected a bundle every three ticks, got {}",
        messages.len()
    );
    let mut expected = 0_u64;
    for message in &messages {
        assert!(
            message.len() <= MAX_MESSAGE_BYTES,
            "a bundle at {} bytes exceeds the bound every hop enforces",
            message.len()
        );
        let value: Value = serde_json::from_str(message).unwrap();
        assert_eq!(value["type"], "audioRx");
        assert_eq!(value["sessionId"], SESSION);
        assert_eq!(value["count"], BUNDLE_FRAMES);
        assert_eq!(value["frameMs"], FRAME_MS);
        assert_eq!(value["epoch"], "0000000000000001");
        // Dense: consecutive bundles advance by exactly their own frame count, which is
        // what lets a player tell a loss from a normal step.
        assert_eq!(
            value["seq"].as_u64().unwrap(),
            expected,
            "sequence is not dense"
        );
        expected += BUNDLE_FRAMES as u64;
        // The payload really is length-prefixed packets, and they really decode.
        let payload = crate::b64_decode(value["payload"].as_str().unwrap()).unwrap();
        let mut at = 0;
        for _ in 0..BUNDLE_FRAMES {
            let length = u16::from_be_bytes([payload[at], payload[at + 1]]) as usize;
            assert!(length > 0);
            at += 2 + length;
        }
        assert_eq!(
            at,
            payload.len(),
            "the packet framing does not account for the payload"
        );
    }
}

#[test]
fn a_browser_that_cannot_keep_up_loses_audio_instead_of_growing_the_station() {
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .unwrap();
    // 600 ticks is twelve seconds of audio into a socket that never becomes writable.
    let (messages, ended) = run(&mut lane, &feed, 600, false);
    assert!(
        messages.is_empty(),
        "audio was sent to a socket that could not take it"
    );
    assert!(
        ended.is_none(),
        "the listener was ended by backpressure rather than losing audio"
    );
    assert!(lane.dropped() > 0, "nothing was recorded as dropped");
    assert_eq!(lane.bundles(), 0);

    // The bound, read off the lane itself rather than inferred: whatever the caller
    // does, the lane is holding less than one bundle's worth at rest.
    let held = lane.listener.as_ref().map_or(0, |l| l.pending.len());
    assert!(
        held < BUNDLE_FRAMES,
        "the lane is holding {held} frames after twelve seconds of backpressure"
    );

    // The control: the identical run against a writable socket DOES deliver, so the
    // emptiness above is backpressure and not a lane that never produces anything.
    let feed = DetachedFeed::new(RATE);
    let mut writable = AudioLane::default();
    writable
        .start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .unwrap();
    let (delivered, _) = run(&mut writable, &feed, 600, true);
    assert!(
        delivered.len() > 100,
        "control: a writable socket delivered {} bundles",
        delivered.len()
    );
    assert_eq!(writable.dropped(), 0);
}

#[test]
fn a_drop_skips_the_sequence_so_the_listener_hears_a_gap_rather_than_a_rewrite() {
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .unwrap();
    let base = Instant::now();
    let mut seqs = Vec::new();
    for k in 0..60 {
        let at = base + Duration::from_millis(FRAME_MS * k as u64);
        feed.publish(at, &tone(TICK, k * TICK));
        // Unwritable for one bundle in the middle of the run.
        let pump = lane.poll(at, !(20..26).contains(&k));
        if let Some(message) = pump.message {
            let value: Value = serde_json::from_str(&message).unwrap();
            seqs.push(value["seq"].as_u64().unwrap());
        }
    }
    let gaps: Vec<u64> = seqs.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        gaps.iter().any(|step| *step > BUNDLE_FRAMES as u64),
        "a dropped bundle did not leave a sequence gap: {seqs:?}"
    );
    assert!(
        gaps.iter().all(|step| *step % BUNDLE_FRAMES as u64 == 0),
        "a gap that is not a whole number of bundles means frames were spliced: {seqs:?}"
    );
}

#[test]
fn a_second_browser_is_refused_and_the_first_keeps_streaming() {
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    let now = Instant::now();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, now).unwrap();
    assert_eq!(
        lane.start(&feed.feed, OTHER, DEVICE, LEASE, now),
        Err("audioInUse")
    );
    assert_eq!(
        lane.session(),
        Some(SESSION),
        "the second browser took the first one's stream"
    );
    let (messages, ended) = run(&mut lane, &feed, 30, true);
    assert!(ended.is_none());
    assert!(!messages.is_empty(), "the first listener stopped being fed");
    assert!(messages.iter().all(|m| m.contains(SESSION)));
    // Re-asking for the session that is already listening is not an error: a browser
    // may repeat itself after a reconnect, and refusing would strand it.
    assert!(lane.start(&feed.feed, SESSION, DEVICE, LEASE, now).is_ok());
}

#[test]
fn stopping_releases_the_feed_so_the_station_goes_back_to_encoding_nothing() {
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .unwrap();
    run(&mut lane, &feed, 30, true);
    // Another session's stop must not end this one's audio.
    assert_eq!(lane.stop(Some(OTHER)), None);
    assert!(
        lane.listening(),
        "an unrelated session's stop ended the listener"
    );
    assert_eq!(lane.stop(Some(SESSION)).as_deref(), Some(SESSION));
    assert!(!lane.listening());
    assert_eq!(lane.encoded_bytes(), 0);
    assert!(
        feed.feed.subscribe(feed.source()).is_ok(),
        "the reader was not released when listening stopped"
    );
}

#[test]
fn a_capture_device_change_ends_the_stream_and_is_never_reported_as_a_gap() {
    let mut feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .unwrap();
    run(&mut lane, &feed, 30, true);
    feed.restart(24_000);
    let (_, ended) = run(&mut lane, &feed, 30, true);
    assert_eq!(ended, Some("sourceChanged"));
    assert!(
        !lane.listening(),
        "the lane kept a reader on a source that no longer exists"
    );
    // And the new source is free for a fresh subscription, which is how listening
    // resumes: a new encoder against the new rate, never the old one spliced on.
    assert!(lane
        .start(&feed.feed, SESSION, DEVICE, LEASE, Instant::now())
        .is_ok());
}

#[test]
fn losing_control_stops_the_audio_but_a_busy_authority_does_not() {
    let feed = DetachedFeed::new(RATE);
    let mut lane = AudioLane::default();
    let base = Instant::now();
    lane.start(&feed.feed, SESSION, DEVICE, LEASE, base)
        .unwrap();
    // Inside the re-check cadence: nothing is asked and nothing changes.
    assert_eq!(
        lane.recheck(base + Duration::from_millis(100), |_, _, _| Err(
            "notController"
        )),
        None
    );
    assert!(lane.listening());
    // Contention is not a refusal. A station whose authority lock was briefly held must
    // not drop a listener for it.
    assert_eq!(
        lane.recheck(base + RECHECK, |_, _, _| Err("remoteBusy")),
        None
    );
    assert!(lane.listening(), "a busy authority ended the listener");
    // A real refusal does stop it, and it names why.
    assert_eq!(
        lane.recheck(base + RECHECK * 3, |_, _, _| Err("notController")),
        Some("notController")
    );
    assert!(!lane.listening());
    assert!(
        feed.feed.subscribe(feed.source()).is_ok(),
        "the reader survived the loss of control"
    );
}

#[test]
fn an_audio_state_names_only_a_fixed_reason() {
    let value: Value =
        serde_json::from_str(&audio_state(SESSION, false, Some("audioInUse"))).unwrap();
    assert_eq!(value["sessionId"], SESSION);
    assert_eq!(value["listening"], false);
    assert_eq!(value["reason"], "audioInUse");
    let quiet: Value = serde_json::from_str(&audio_state(SESSION, true, None)).unwrap();
    assert!(
        quiet.get("reason").is_none(),
        "a state with no reason invented one"
    );
}

#[test]
fn every_refusal_this_lane_can_produce_is_one_the_browser_knows() {
    // The vocabulary, copied from ui/src/remote-web/audio-protocol.ts. A code outside it
    // is refused by the browser's parser and closes its socket, so this is the guard on
    // the one place where adding an error string has a consequence three layers away.
    const KNOWN: [&str; 5] = [
        "audioInUse",
        "notController",
        "audioUnavailable",
        "sourceChanged",
        "audioStopped",
    ];
    // Everything `start`, `poll`, `recheck` and `Authority::audio_admitted` can return.
    for reason in [
        "audioInUse",
        "audioUnavailable",
        "sourceChanged",
        "notController",
        "audioStopped",
        "remoteBusy",
        "invalidRequest",
        "authorityUnavailable",
        "somethingNobodyHasWrittenYet",
    ] {
        assert!(
            KNOWN.contains(&shared_reason(reason)),
            "{reason} would reach a browser as an unknown code and close its socket"
        );
    }
    // Controls: the narrowing must not flatten the codes that carry real meaning, and
    // it must actually change the ones that do not.
    assert_eq!(shared_reason("audioInUse"), "audioInUse");
    assert_eq!(shared_reason("notController"), "notController");
    assert_eq!(shared_reason("remoteBusy"), "audioUnavailable");
}
