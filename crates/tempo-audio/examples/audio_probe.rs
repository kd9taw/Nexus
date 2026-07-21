//! Multi-radio Phase 0 — does Nexus's OWN cpal path hold two radios' codecs open at once?
//!
//! Two WSJT-X instances running two USB codecs proves nothing about this: those are two
//! PROCESSES on PortAudio/Qt. This probe answers the actual question — one process,
//! `tempo-audio`'s `CpalBackend`, both of the operator's enabled radio profiles open
//! SIMULTANEOUSLY, capture AND playback, on his real settings.json.
//!
//!   cargo run -p tempo-audio --features device,serial --example audio_probe
//!   cargo run -p tempo-audio --features device,serial --example audio_probe -- <settings.json>
//!
//! It is deliberately an example and not a flag on the shipped binary: examples are not
//! compiled into `Nexus`, so this code can never run on an operator's station by accident,
//! and unlike `src-tauri` (a standalone non-workspace crate that needs a WebView toolchain)
//! it builds and lints on the dev box.
//!
//! WARNING — this plays a 1 kHz tone into each rig's audio input. A rig with VOX enabled
//! WILL transmit. The probe asks before the tone phase.

use std::f32::consts::PI;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tempo_app::settings::Settings;
use tempo_audio::backend::AudioBackend;
use tempo_audio::device::{available_devices, CpalBackend};

/// The rate `CpalBackend::capture`/`play` speak, whatever the device runs at.
const MODEM_RATE: usize = 12_000;
const TONE_HZ: f32 = 1000.0;
const TONE_SECS: u64 = 2;
/// Concurrent-RX phase length: long enough for both meters to settle on band noise.
const RX_SECS: u64 = 10;

/// One open radio chain: the profile name, its backend, and the dropout tally.
struct Chain {
    name: String,
    backend: CpalBackend,
    stalls: u32,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(settings_path);
    println!("Nexus audio probe — multi-radio Phase 0");
    // Nexus (or WSJT-X) already holding a codec turns a working card into a failed open —
    // a false NEGATIVE on the one question this probe exists to answer.
    println!("CLOSE Nexus and any other radio software first, or the result is meaningless.");
    println!("settings: {}", path.display());
    if !path.exists() {
        eprintln!("no settings file there — pass the path as the first argument");
        std::process::exit(2);
    }
    // Parse READ-ONLY. Settings::load() renames an unparseable file to .json.corrupt and
    // returns defaults (settings.rs:1648) — after which Nexus would save those defaults back
    // over the path, resetting license_class to Open and silently re-opening TX privileges.
    // A diagnostic must never be able to do that to a live station config.
    let settings: Settings = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(s) => s,
        None => {
            eprintln!(
                "could not parse {} — leaving it untouched, nothing written",
                path.display()
            );
            std::process::exit(2);
        }
    };
    let mut settings = settings;
    settings.ensure_radio_profiles(); // the one normalization step load() would have done
    let radios: Vec<_> = settings.radios.iter().filter(|p| p.enabled).collect();
    println!(
        "enabled radio profiles: {}",
        radios
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if radios.len() < 2 {
        eprintln!(
            "\nNOT A TEST: {} enabled radio profile(s). Configure and enable two radios \
             (each with its OWN audio in/out device) in Settings first.",
            radios.len()
        );
        std::process::exit(2);
    }

    // The picker's view of the machine. `available_devices` disambiguates identical names —
    // two rigs that both enumerate as "USB Audio CODEC" appear as that and "USB Audio CODEC #2",
    // which is exactly the operator's hardware and exactly what must end up in settings.json.
    let (ins, outs) = available_devices();
    println!("\ninput devices:");
    for (i, d) in ins.iter().enumerate() {
        println!("  [{i}] {d}");
    }
    println!("output devices:");
    for (i, d) in outs.iter().enumerate() {
        println!("  [{i}] {d}");
    }

    // Pre-flight. `CpalBackend::open` falls back to the SYSTEM DEFAULT device when a name
    // matches nothing, so an unresolvable (or blank) name would quietly point both chains at
    // one device and the probe would report a PASS it did not earn. Refuse instead — the same
    // no-silent-fallback rule `CaptureStream::open` enforces for the voice mic.
    let mut refused = false;
    for p in &radios {
        for (kind, want, list) in [
            ("in", p.audio_in.trim(), &ins),
            ("out", p.audio_out.trim(), &outs),
        ] {
            if want.is_empty() {
                println!("\nREFUSED: {} has no audio {kind} device set.", p.name);
                refused = true;
            } else if let Some(i) = list.iter().position(|d| d == want) {
                println!("\n{} audio {kind}: {want:?} → device [{i}]", p.name);
            } else {
                println!(
                    "\nREFUSED: {}'s audio {kind} {want:?} is not on this machine \
                     (pick the exact name from the list above).",
                    p.name
                );
                refused = true;
            }
        }
    }
    if let Some(msg) = tempo_app::settings::audio_device_conflicts(&settings.radios) {
        println!("\nREFUSED: {msg}");
        refused = true;
    }
    if refused {
        eprintln!("\nprobe not run — fix the audio configuration above first");
        std::process::exit(2);
    }

    // ---- the actual question: N backends alive at the same time ----
    let mut chains: Vec<Chain> = Vec::new();
    for p in &radios {
        print!("\nopening {} … ", p.name);
        let _ = std::io::stdout().flush();
        match CpalBackend::open(Some(p.audio_in.trim()), Some(p.audio_out.trim())) {
            Ok(mut backend) => {
                // The operator's own levels, so this also exercises his real TX drive / RX gain.
                backend.set_tx_level(p.tx_level);
                backend.set_rx_gain(p.rx_gain);
                println!("OK");
                chains.push(Chain {
                    name: p.name.clone(),
                    backend,
                    stalls: 0,
                });
            }
            Err(e) => {
                println!("FAILED: {e}");
                eprintln!(
                    "\nNEGATIVE RESULT: this process could not hold {} codec(s) open at once. \
                     That is the Phase 0 answer — report it, do not work around it.",
                    chains.len() + 1
                );
                std::process::exit(1);
            }
        }
    }
    println!(
        "\nall {} codecs open in ONE process. Watch stderr for \
         \"tempo-audio: cpal stream error\" lines — a stream that dies later is still a failure.",
        chains.len()
    );

    // Prime: discard whatever queued while the later streams were still opening.
    for c in chains.iter_mut() {
        c.backend.capture();
    }

    println!("\n== concurrent RX ({RX_SECS} s) — both meters must move on live band noise ==");
    meter(&mut chains, RX_SECS);

    println!(
        "\n== 1 kHz TONE ==\n\
         This feeds audio into each rig's data/mic input. A rig on VOX WILL TRANSMIT.\n\
         Turn VOX off, or make sure the antenna/dummy load and power are safe."
    );
    print!("type 'tone' then Enter to run the tone phase (anything else skips it): ");
    let _ = std::io::stdout().flush();
    // Read the RESULT, and require an explicit word — not bare Enter. A discarded Result
    // makes EOF indistinguishable from consent, so `audio_probe | tee log.txt`, `< /dev/null`
    // or `nohup` would feed a 1 kHz tone into the rig's mic input unattended, and a rig on
    // VOX TRANSMITS. Non-interactive stdin must mean SKIP, never proceed.
    // Read stdin on a worker while the main thread keeps DRAINING the capture rings. The
    // rings are unbounded (device.rs in_ring) and only the radio loop drains them in the real
    // app; here nothing would, so a slow operator at this prompt could accumulate gigabytes
    // and then hang for minutes inside the polyphase resampler on the next drain. That looks
    // identical to "one process cannot hold two codecs" — the exact false negative this whole
    // probe exists to rule out.
    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let mut l = String::new();
        let _ = tx.send(match std::io::stdin().read_line(&mut l) {
            Ok(0) | Err(_) => None, // EOF / error — never consent
            Ok(_) => Some(l),
        });
    });
    let answer = loop {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(a) => break a,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                for c in chains.iter_mut() {
                    c.backend.capture(); // keep the rings bounded while we wait
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break None,
        }
    };
    let line = answer.unwrap_or_default();
    let consented = !line.is_empty() && line.trim().eq_ignore_ascii_case("tone");
    if !consented {
        println!("tone phase SKIPPED (no explicit confirmation) — RX results above still stand.");
        return;
    }
    // Nothing drained the capture rings while that prompt blocked, so discard the backlog —
    // otherwise the first tone-phase sample count reports however long the operator took.
    for c in chains.iter_mut() {
        c.backend.capture();
    }

    let tone = tone(TONE_SECS);
    for i in 0..chains.len() {
        println!("\n-- tone → {} ONLY --", chains[i].name);
        chains[i].backend.play(&tone);
        meter(&mut chains, TONE_SECS + 1);
    }
    println!("\n-- tone → ALL RADIOS AT ONCE --");
    for c in chains.iter_mut() {
        c.backend.play(&tone);
    }
    meter(&mut chains, TONE_SECS + 1);

    println!(
        "\n== SOAK — leave this running 15 minutes. Ctrl-C to stop. ==\n\
         A chain that stops delivering ~{MODEM_RATE} samples/s has lost its device."
    );
    meter(&mut chains, u64::MAX);
}

/// Print one line per second for `secs` seconds: each chain's RX level and how many 12 kHz
/// samples it actually delivered. The sample count is the part that matters — a dead stream
/// reads the same 0 dB as a silent one, but it stops delivering samples.
fn meter(chains: &mut [Chain], secs: u64) {
    for t in 1..=secs {
        let start = Instant::now();
        std::thread::sleep(Duration::from_secs(1));
        let elapsed = start.elapsed().as_secs_f32();
        let mut line = format!("t={t:>4}s ");
        for c in chains.iter_mut() {
            let n = c.backend.capture().len();
            let rate = n as f32 / elapsed.max(0.001);
            let stalled = rate < MODEM_RATE as f32 * 0.5;
            if stalled {
                c.stalls += 1;
            }
            line.push_str(&format!(
                " | {:<12} {:>5.1} dB {:>6.0} samp/s{}",
                c.name,
                db(c.backend.rx_level()),
                rate,
                if stalled { "  *** STALLED" } else { "" },
            ));
        }
        println!("{line}");
    }
    let stalls: u32 = chains.iter().map(|c| c.stalls).sum();
    if stalls > 0 {
        println!("  !! {stalls} stalled second(s) so far — see the per-chain marks above");
    }
}

/// WSJT-X-style level, the same formula the UI meter renders (`ui/src/components/LevelMeter.tsx`).
fn db(rms: f32) -> f32 {
    if rms <= 0.0 {
        0.0
    } else {
        (20.0 * rms.log10() + 90.3).max(0.0)
    }
}

/// `secs` seconds of 1 kHz at the modem rate, half scale (the backend then applies the
/// profile's own `tx_level`).
fn tone(secs: u64) -> Vec<f32> {
    let n = MODEM_RATE * secs as usize;
    (0..n)
        .map(|i| (2.0 * PI * TONE_HZ * i as f32 / MODEM_RATE as f32).sin() * 0.5)
        .collect()
}

/// Where the app persists settings — mirrors `settings_path()` in `src-tauri/src/lib.rs`,
/// which this crate cannot reach (src-tauri is a standalone, non-workspace crate).
fn settings_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("tempo")
        .join("settings.json")
}
