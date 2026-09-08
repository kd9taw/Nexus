//! Readers for the two sanitised JS8Call log fixtures (Task B4.1).
//!
//! `js8call_frames.txt` — one ALL.TXT decode line per row: `<letter> <snr> <dt> <freq_hz>
//! <sixbit12> <i3> <rendered text>`; the rendered text is EVERYTHING after the sixth space,
//! trailing spaces included (they are wire truth: JS8Call renders "CALL: @HB HEARTBEAT FN30 ").
//! `# +N` lines advance a relative clock by N seconds; other `#` lines are comments.
//! `js8call_directed.txt` — `<offset_hz>\t<snr>\t<text>` (the DIRECTED.TXT line minus its
//! date, dial and ♢ marker).
//!
//! WHY the clock is in the fixture: the reassembly oracle (golden_directed) must replay
//! JS8Call's 60 s force-close / 90 s drop rules on the real cadence, and the fixture carries
//! no dates by design (spec G5). Both readers call `verify_fixture_pins()` first, so a
//! swapped or truncated fixture is a red test, never a quiet drift.
use js8::Speed;

/// One decoded frame as JS8Call logged it.
#[derive(Debug, Clone, PartialEq)]
pub struct GoldenFrame {
    /// Milliseconds since the first frame of the fixture (from the `# +N` clock lines).
    pub at_ms: u64,
    pub speed: Speed,
    pub snr_db: i32,
    pub dt_s: f32,
    pub freq_hz: f32,
    /// The twelve 6-bit chars as JS8Call printed them (alphabet `0-9A-Za-z-+`).
    pub sixbit: [u8; 12],
    pub i3: u8,
    /// JS8Call's rendered line — the value `Frame::render` must reproduce byte for byte, or the
    /// raw 12 chars when JS8Call itself could not unpack the frame.
    pub text: String,
}

impl GoldenFrame {
    /// The 12-char field as text — the input `js8::proto::alphabet::sixbit_from_str` takes.
    pub fn sixbit_str(&self) -> &str {
        std::str::from_utf8(&self.sixbit).expect("the sanitiser only emits ASCII sixbit fields")
    }
}

pub fn read_frames() -> Vec<GoldenFrame> {
    super::verify_fixture_pins();
    let raw = std::fs::read_to_string(super::fixture_dir().join("js8call_frames.txt"))
        .expect("crates/js8/tests/fixtures/js8call_frames.txt (Task B4.1)");
    let mut out = Vec::new();
    let mut clock_ms: u64 = 0;
    for (n, line) in raw.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("# +") {
            let secs: u64 = rest
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("line {}: bad clock line {line:?}", n + 1));
            clock_ms += secs * 1000;
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(7, ' ');
        let mut next = |what: &str| {
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing {what} in {line:?}", n + 1))
                .to_string()
        };
        let letter = next("letter");
        let snr = next("snr");
        let dt = next("dt");
        let freq = next("freq");
        let sixbit = next("sixbit");
        let i3 = next("i3");
        let text = parts.next().unwrap_or("").to_string();
        let mut sb = [0u8; 12];
        assert_eq!(
            sixbit.len(),
            12,
            "line {}: sixbit field is not 12 chars",
            n + 1
        );
        sb.copy_from_slice(sixbit.as_bytes());
        out.push(GoldenFrame {
            at_ms: clock_ms,
            speed: Speed::from_letter(letter.chars().next().unwrap())
                .unwrap_or_else(|| panic!("line {}: bad letter {letter}", n + 1)),
            snr_db: snr.parse().unwrap(),
            dt_s: dt.parse().unwrap(),
            freq_hz: freq.parse().unwrap(),
            sixbit: sb,
            i3: i3.parse().unwrap(),
            text,
        });
    }
    out
}

/// One DIRECTED.TXT line as JS8Call logged it (marker and trailing whitespace removed).
#[derive(Debug, Clone, PartialEq)]
pub struct GoldenDirected {
    pub freq_hz: f32,
    pub snr_db: i32,
    pub text: String,
}

pub fn read_directed() -> Vec<GoldenDirected> {
    super::verify_fixture_pins();
    let raw = std::fs::read_to_string(super::fixture_dir().join("js8call_directed.txt"))
        .expect("crates/js8/tests/fixtures/js8call_directed.txt (Task B4.1)");
    raw.lines()
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(n, line)| {
            let mut cols = line.splitn(3, '\t');
            let freq = cols
                .next()
                .unwrap_or_else(|| panic!("line {}: no offset", n + 1));
            let snr = cols
                .next()
                .unwrap_or_else(|| panic!("line {}: no snr", n + 1));
            let text = cols.next().unwrap_or("");
            GoldenDirected {
                freq_hz: freq.parse().unwrap(),
                snr_db: snr.parse().unwrap(),
                text: text.to_string(),
            }
        })
        .collect()
}
