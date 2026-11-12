//! Phone's Lock here: the same held press on Stop's road, bounded by its cap.
use super::*;
use crate::remote_service::lan::stop_road::Up;
use std::time::Instant;

/// A cap a test can wait for, asked about half way.
fn short_lock(client: &Client) {
    client.time_with(Timing {
        lock_cap: Duration::from_millis(1200),
        lock_ask: Duration::from_millis(600),
        ..Timing::default()
    });
}

fn hold(s: &Screens, still_talking: bool) -> Result<Value, Value> {
    ask(
        &s.main,
        "hold_ptt_lock",
        json!({ "stillTalking": still_talking }),
    )
}

/// One hold's answer, with when (after the test's `from`) its call was made and when it returned.
/// The client reads its clock somewhere between the two, so a test bounds the Lock's age with
/// both: an answer stamped only when it came read a call delayed on a loaded box as made late.
type Said = (Duration, Duration, Result<Value, Value>);

/// Holds the Lock every 50 ms until `until` after `from`, each answer kept with its call's times.
fn hold_until(s: &Screens, from: Instant, until: Duration) -> Vec<Said> {
    let mut said = Vec::new();
    while from.elapsed() < until {
        let made = from.elapsed();
        let answer = hold(s, false);
        said.push((made, from.elapsed(), answer));
        std::thread::sleep(Duration::from_millis(50));
    }
    said
}

/// ★ The Lock here is the same held press on Stop's road as the PTT held: each re-assertion is one
/// hold of one press, the screen's alone, and nothing reaches the link; the Lock let go (the PTT's
/// off half, as Phone sends it) is its release on the road. Under the cap, the answer asks nothing.
/// CONTROL: the next Lock is a new press.
#[test]
fn a_lock_here_is_the_same_held_press_on_stop_road() {
    let (shack, end, _client, s, dir) = present_for(Duration::from_secs(5));
    for _ in 0..3 {
        assert_eq!(hold(&s, false), Ok(json!({ "ask": false })));
        std::thread::sleep(Duration::from_millis(100));
    }
    eventually("three holds", || end.presses().len() >= 3);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        end.presses(),
        [
            Up::Hold { press: 1, stops: 0 },
            Up::Hold { press: 1, stops: 0 },
            Up::Hold { press: 1, stops: 0 }
        ],
        "one hold a re-assertion, none of its own"
    );
    assert!(shack.sent().is_empty(), "{:?}", shack.sent());
    assert!(ask(&s.main, "set_ptt", json!({"on": false})).is_ok());
    eventually("the release", || end.presses().len() >= 4);
    assert_eq!(end.presses()[3], Up::Release { press: 1 });
    assert_eq!(shack.sent(), ["set_ptt"], "the off half on the link");
    assert!(hold(&s, false).is_ok());
    eventually("the next Lock", || end.presses().len() >= 5);
    assert_eq!(end.presses()[4], Up::Hold { press: 2, stops: 1 });
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ⛔ A Lock nobody answers ends at its cap. Held with no answer, it is not asked before the cap
/// less the question's lead, is asked with the time left after it, and the first hold at the cap
/// (an answer included, which comes too late) is refused `lockCapped`, the press let go on its road.
/// It never keys again: every re-assertion after is `pressEnded`, with no hold on the road, until
/// the screen lets go. CONTROL: a PTT held, not locked, goes on past the same cap.
#[test]
fn a_lock_nobody_answers_ends_at_its_cap() {
    let (_shack, end, client, s, dir) = present_for(Duration::from_secs(10));
    short_lock(&client);
    let from = Instant::now();
    let said = hold_until(&s, from, Duration::from_millis(1100));
    // The Lock began while the first hold was handled: after `from`, and by that call's return.
    let began_by = said[0].1;
    let mut asked = 0;
    for (made, returned, answer) in &said {
        if *returned < Duration::from_millis(550) {
            // At most `returned` old when read: under the 600 ms the question waits.
            let answer = answer.as_ref().unwrap_or_else(|e| panic!("{made:?}: {e}"));
            assert_eq!(answer, &json!({ "ask": false }), "{made:?}");
        } else if *made >= began_by + Duration::from_millis(650)
            && *returned < Duration::from_millis(1150)
        {
            // At least 650 ms old when read, and under its 1200 ms cap.
            let answer = answer.as_ref().unwrap_or_else(|e| panic!("{made:?}: {e}"));
            assert_eq!(answer["ask"], true, "{made:?}: {answer}");
            let left = answer["leftMs"].as_u64().unwrap();
            assert!(left <= 600 && left > 0, "{made:?}: {answer}");
            asked += 1;
        }
    }
    assert!(asked > 0, "CONTROL: a hold was asked: {said:?}");
    // At least 1250 ms old when this hold is read: past its cap.
    while from.elapsed() < began_by + Duration::from_millis(1250) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        hold(&s, true),
        refused("lockCapped"),
        "an answer at the cap is too late"
    );
    eventually("let go on the road", || {
        end.presses().last() == Some(&Up::Release { press: 1 })
    });
    let holds = end.presses().len();
    assert_eq!(hold(&s, true), refused("pressEnded"));
    assert_eq!(hold(&s, false), refused("pressEnded"));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(end.presses().len(), holds, "no hold after the cap");
    // CONTROL: let go, and a PTT held (not locked) over the same stretch is never capped.
    assert!(ask(&s.main, "set_ptt", json!({"on": false})).is_ok());
    let from = Instant::now();
    while from.elapsed() < Duration::from_millis(1600) {
        assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(matches!(
        end.presses().last(),
        Some(Up::Hold { press: 2, .. })
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ An answer to "Still talking?" moves the cap on: answered while asked, the Lock is no longer
/// asked, goes on the air past the cap it would have met, and is capped only a whole cap after the
/// answer. It goes on under a new press, since the station caps each press on its own clock: the
/// answer's hold names the next press, under the Stops pressed so far, and the first press is never
/// let go, so the station hands the key straight over; the cap lets go the new one. CONTROL: the
/// same Lock unanswered was asked before the answer, and held press 1 until then.
#[test]
fn an_answer_to_still_talking_moves_the_cap_on() {
    let (_shack, end, client, s, dir) = present_for(Duration::from_secs(10));
    short_lock(&client);
    let from = Instant::now();
    let said = hold_until(&s, from, Duration::from_millis(800));
    assert!(
        said.iter()
            .any(|(.., answer)| answer.as_ref().is_ok_and(|a| a["ask"] == true)),
        "CONTROL: asked: {said:?}"
    );
    assert!(
        end.presses()
            .iter()
            .all(|up| *up == Up::Hold { press: 1, stops: 0 }),
        "CONTROL: {:?}",
        end.presses()
    );
    let answered = Instant::now();
    assert_eq!(hold(&s, true), Ok(json!({ "ask": false })), "answered");
    // The answer moved the count on while it was handled: after `answered`, and by its return.
    let moved_by = answered.elapsed();
    eventually("the next press", || {
        end.presses().last() == Some(&Up::Hold { press: 2, stops: 0 })
    });
    let said = hold_until(&s, answered, Duration::from_millis(1100));
    // Every hold read under 1150 ms after the answer is under the moved cap.
    assert!(
        said.iter()
            .filter(|(_, returned, _)| *returned < Duration::from_millis(1150))
            .all(|(.., answer)| answer.is_ok()),
        "on the air past the first cap: {said:?}"
    );
    assert!(
        from.elapsed() > Duration::from_millis(1700),
        "premise: past the first cap"
    );
    while answered.elapsed() < moved_by + Duration::from_millis(1250) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        hold(&s, false),
        refused("lockCapped"),
        "a whole cap after the answer"
    );
    eventually("let go on the road", || {
        end.presses().last() == Some(&Up::Release { press: 2 })
    });
    assert!(
        !end.presses().contains(&Up::Release { press: 1 }),
        "the first press let go: {:?}",
        end.presses()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
