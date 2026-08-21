//! Dump raw FT-710 spectrum frames to a file, for offline structure analysis.
//!
//! WHY: `read_frame` takes a fixed 4096-byte SPI read with no framing check, so nothing verifies
//! that a read starts at a frame boundary. If the radio's stream can slip relative to our reads,
//! every bin lands at the wrong bin index and the spectrum is scrambled — which is what the
//! operator sees after changing the rig's scope mode and changing it back (2026-08-20). Before
//! adding a sync check, find out whether the frame HAS anything to sync on.
//!
//!   DYLD_LIBRARY_PATH=$LIB cargo run -p tempo-audio --features yaesu-wf \
//!     --example yaesu_wf_dump -- /tmp/frames.bin 200
//!
//! Writes N frames back-to-back. Nothing is sent to the radio.
use tempo_audio::yaesu_wf::{ft4222::Ft4222Waterfall, WaterfallSource, FRAME_BYTES};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "/tmp/frames.bin".into());
    let count: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(100);
    let mut src = match Ft4222Waterfall::open(0) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FT4222 open failed: {e} (is Nexus holding it?)");
            std::process::exit(1);
        }
    };
    // Optional third arg: pause this many milliseconds after every 100 frames, to imitate what the
    // app does while the rig's scope is in FIX/CURSOR — `pump` returns Unavailable BEFORE reading,
    // so the stream is not drained at all for as long as that lasts.
    let pause_ms: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut out = Vec::with_capacity(count * FRAME_BYTES);
    let mut short = 0usize;
    for _ in 0..count {
        match src.read_frame() {
            Ok(f) => {
                if f.len() != FRAME_BYTES {
                    short += 1;
                }
                out.extend_from_slice(&f);
            }
            Err(e) => eprintln!("read failed: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(12));
        if pause_ms > 0 && out.len() % (100 * FRAME_BYTES) == 0 {
            eprintln!("pausing {pause_ms} ms after {} frames", out.len() / FRAME_BYTES);
            std::thread::sleep(std::time::Duration::from_millis(pause_ms));
        }
    }
    std::fs::write(&path, &out).expect("write");
    println!("{} bytes to {path} ({} short reads)", out.len(), short);
}
