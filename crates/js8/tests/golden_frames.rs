//! Golden test of the JS8 frame codec against JS8Call's own ALL.TXT (sanitised, Task B4.1).
//! Task B4.1 lands only the fixture-reader gate; Task B4.4 adds the codec assertions.
mod common;

use common::golden::{read_directed, read_frames};
use js8::Speed;

#[test]
fn frames_fixture_reads_with_the_expected_shape() {
    let frames = read_frames();
    // The oracle cannot be silently truncated: floors per speed as measured 2026-09-05
    // (965 A / 341 B / 27 E). The pin in SHA256SUMS is the exact truth.
    let n = |s: Speed| frames.iter().filter(|f| f.speed == s).count();
    assert!(frames.len() >= 1333, "only {} frames", frames.len());
    assert!(
        n(Speed::Normal) >= 965 && n(Speed::Fast) >= 341 && n(Speed::Slow) >= 27,
        "A={} B={} E={}",
        n(Speed::Normal),
        n(Speed::Fast),
        n(Speed::Slow)
    );
    assert_eq!(
        n(Speed::Turbo),
        0,
        "no Turbo line existed in the source log; a Turbo row means a different log was sanitised"
    );
    assert_eq!(frames[0].at_ms, 0);
    assert!(
        frames.last().unwrap().at_ms > 24 * 3600 * 1000,
        "the clock lines did not accumulate"
    );
    let anchor = frames
        .iter()
        .find(|f| &f.sixbit == b"2Wu+WEWCuiZG")
        .expect("the KD2UWR heartbeat anchor");
    assert_eq!(anchor.i3, 3);
    assert_eq!(anchor.text, "KD2UWR: @HB HEARTBEAT FN30 ");
    let trailing = frames.iter().filter(|f| f.text.ends_with(' ')).count();
    assert!(
        trailing > 500,
        "trailing spaces were stripped somewhere: {trailing}"
    );
}

#[test]
fn directed_fixture_reads_with_the_expected_shape() {
    let rows = read_directed();
    assert!(rows.len() >= 728, "only {} directed lines", rows.len());
    assert!(rows
        .iter()
        .all(|r| !r.text.contains('\u{2662}') && !r.text.ends_with(' ')));
    assert!(rows.iter().any(|r| r.text == "KD2UWR: @HB HEARTBEAT"));
    assert!(rows.iter().any(|r| r.text.contains(" HEARTBEAT SNR ")));
}
