//! Live proof of the FT-710 waterfall chain, end to end, outside the app.
//!
//! WHAT THIS IS FOR. The transport, the parsing and the publish are each unit-tested, but one
//! thing cannot be tested without a radio: whether the BINS AND THE METADATA AGREE — whether a
//! signal lands on the frequency the rig says it is on. That is the only question left before this
//! is worth wiring into the radio service, and it is what this prints.
//!
//! Run it and compare the peak it names against the rig's own scope:
//!
//! ```text
//! LIB=$(dirname $(find … -name libft4222.dylib))
//! RUSTFLAGS="-L $LIB" DYLD_LIBRARY_PATH="$LIB" \
//!   cargo run -p tempo-audio --features yaesu-wf --example yaesu_wf_probe
//! ```
//!
//! WHERE THE NUMBERS COME FROM, and the split matters. The DIAL is read from Nexus's own CAT
//! broker (:4532), which answers from its live state and never touches the serial link — so this
//! can poll it freely while Nexus keeps operating. SPAN and MODE need `SS05;`/`SS06;`, which only
//! the real rigctld answers (:4533), so they are read RARELY: hammering that daemon is what made
//! the DSP toggles disappear on 2026-08-17 (three missed func reads mark a func unsupported, and
//! the retry backs off to half an hour).
//!
//! Nothing here writes to the radio.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use tempo_audio::yaesu_wf::{
    ft4222::Ft4222Waterfall, parse_ss_reply, parse_wf1, span_hz, sweep_edges, WaterfallSource,
    WF1_BINS,
};

/// One request/reply against a rigctld-protocol endpoint. `None` on any fault — a probe that
/// cannot ask is not a probe that learned something.
///
/// ⚠️ THE TWO REPLY TERMINATORS. `f` answers with a newline; `w` (send_cmd, the raw-CAT relay)
/// answers with the rig's own string terminated by a NUL — `SS0570000;\0`. Reading a LINE
/// therefore hangs on the raw path until the socket timeout and looks exactly like "the rig did
/// not answer", which is how this cost a debugging round on 2026-08-17 after the same command had
/// just worked through `rigctl`. So: read bytes, stop on either terminator.
fn ask(addr: &str, line: &str) -> Option<String> {
    let mut s = TcpStream::connect_timeout(&addr.parse().ok()?, Duration::from_millis(800)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(800))).ok()?;
    writeln!(s, "{line}").ok()?;
    let mut buf = Vec::new();
    let mut b = [0u8; 64];
    while buf.len() < 256 {
        match s.read(&mut b) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&b[..n]),
        }
        if buf.contains(&b'\n') || buf.contains(&0) {
            break;
        }
    }
    let t = String::from_utf8_lossy(&buf)
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string();
    (!t.is_empty()).then_some(t)
}

/// SPAN and MODE codes, straight off the rig. Read together because they are one question.
fn read_scope_meta(rig: &str) -> Option<(u8, u8)> {
    let span = parse_ss_reply(&ask(rig, "w SS05;")?, b'5')?;
    let mode = parse_ss_reply(&ask(rig, "w SS06;")?, b'6')?;
    Some((span, mode))
}

/// 852 bins into `cols` characters — a crude waterfall line, enough to see a carrier sit still.
fn render(row: &[f32], cols: usize) -> String {
    const RAMP: &[u8] = b" .:-=+*#%@";
    let per = row.len() / cols;
    (0..cols)
        .map(|c| {
            let seg = &row[c * per..((c + 1) * per).min(row.len())];
            let peak = seg.iter().cloned().fold(0.0f32, f32::max);
            // The row is already 0..1; stretch the interesting half so a floor at ~0.7 still
            // shows structure rather than a solid bar.
            let v = ((peak - 0.55) / 0.45).clamp(0.0, 1.0);
            RAMP[(v * (RAMP.len() - 1) as f32) as usize] as char
        })
        .collect()
}

fn main() {
    let broker = "127.0.0.1:4532"; // Nexus's own broker — live dial, no serial contention
    let rig = "127.0.0.1:4533"; // the FT-710's rigctld — SS reads only, rarely

    let Some((mut span_code, mut mode_code)) = read_scope_meta(rig) else {
        eprintln!("could not read SS05;/SS06; from {rig} — is Nexus running with the FT-710 up?");
        return;
    };
    println!(
        "scope: span {:?} Hz, mode code '{}' ({})",
        span_hz(span_code),
        mode_code as char,
        if tempo_audio::yaesu_wf::mode_is_centered(mode_code) {
            "centred on the dial"
        } else {
            "NOT centred — edges unknowable, rows will be refused"
        }
    );

    let mut wf = match Ft4222Waterfall::open(0) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("FT4222 open failed: {e}");
            eprintln!("is SCU-LAN10 enabled in the radio's menu? (that is what makes it appear)");
            return;
        }
    };

    let mut last_meta = Instant::now();
    for i in 0..u32::MAX {
        // Span/mode change only when the operator turns a knob; a slow re-read keeps this honest
        // without pestering the daemon that also serves Nexus.
        if last_meta.elapsed() > Duration::from_secs(10) {
            if let Some((s, m)) = read_scope_meta(rig) {
                (span_code, mode_code) = (s, m);
            }
            last_meta = Instant::now();
        }

        let dial_hz = ask(broker, "f").and_then(|s| s.parse::<f64>().ok());
        let Some(dial_hz) = dial_hz else {
            eprintln!("no dial from the broker — skipping");
            std::thread::sleep(Duration::from_millis(500));
            continue;
        };

        let Ok(raw) = wf.read_frame() else {
            eprintln!("SPI read failed");
            continue;
        };
        let Some(row) = parse_wf1(&raw) else {
            eprintln!("short frame ({} bytes) — dropped", raw.len());
            continue;
        };
        let Some((lo, hi)) = sweep_edges(dial_hz, span_code, mode_code) else {
            eprintln!("cannot place this row (mode '{}')", mode_code as char);
            std::thread::sleep(Duration::from_millis(500));
            continue;
        };

        // THE ACTUAL TEST: where does the strongest bin sit, in MHz? Compare it with the rig.
        let (peak_i, peak_v) =
            row.iter().enumerate().fold(
                (0usize, 0.0f32),
                |acc, (i, &v)| {
                    if v > acc.1 {
                        (i, v)
                    } else {
                        acc
                    }
                },
            );
        let peak_hz = lo + (hi - lo) * (peak_i as f64 / (WF1_BINS - 1) as f64);

        if i % 10 == 0 {
            println!(
                "\ndial {:.6} MHz   span {:.0} kHz   [{:.4} … {:.4} MHz]",
                dial_hz / 1e6,
                (hi - lo) / 1e3,
                lo / 1e6,
                hi / 1e6
            );
        }
        println!(
            "{}  peak {:.4} MHz ({:.2})",
            render(&row, 96),
            peak_hz / 1e6,
            peak_v
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}
