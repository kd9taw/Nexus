# tempo-sstv

Nexus SSTV receiver core (pure decode — no audio I/O, no UI, raw RGB
buffers out).

## Provenance

Vendored from the MIT-licensed `slowrx` crate **v0.5.3**
(<https://github.com/jasonherald/slowrx.rs>, release commit `aa384b4`,
byte-identical to the crates.io tarball), which is itself a pure-Rust port
of [slowrx](https://github.com/windytan/slowrx) by Oona Räisänen (OH2EIQ),
ISC license. Full attribution chain in `NOTICE.md`; this crate stays MIT
(GPL-compatible) inside the GPL-3.0-only workspace.

Vendored rather than taken as a Cargo dependency so the tree is
self-contained and auditable (young single-author upstream). To diff
against upstream: `src/` matches the 0.5.3 release except for the local
changes below.

## Local changes vs upstream 0.5.3

- Crate renamed `slowrx` → `tempo-sstv`; CLI binary, `decode_wav` example
  and the `cli` feature (hound/image/clap/anyhow deps) dropped — Nexus
  renders from the raw event stream.
- **PD50 / PD90 / PD160 / PD290 added** to the mode table (`modespec.rs`),
  timings row-for-row from slowrx C `modespec.c`; the shared PD decode
  path handles them unchanged. Upstream baseline already had PD120 (the
  ISS mode) / PD180 / PD240, Robot 24/36/72, Scottie 1/2/DX, Martin 1/2.
- `test-support` is enabled for tests via a self-dev-dependency so plain
  `cargo test -p tempo-sstv` runs the whole suite (no `--features` flag).
- `tests/nexus_acceptance.rs` added: an independent test-local modulator
  (real on-air VIS preamble, hardcoded published timings) feeds the
  decoder at 12 kHz in 1024-sample streaming chunks; decoded Scottie 1
  and PD120 images must correlate > 0.9 with the source, plus
  unknown-VIS / noise rejection.

## Use

```rust
let mut d = tempo_sstv::SstvDecoder::new(12_000)?; // caller's sample rate
for ev in d.process(&audio_chunk) {
    match ev {
        tempo_sstv::SstvEvent::VisDetected { mode, .. } => { /* arm UI */ }
        tempo_sstv::SstvEvent::LineDecoded { line_index, pixels, .. } => { /* progressive render */ }
        tempo_sstv::SstvEvent::ImageComplete { image, .. } => { /* gallery */ }
        _ => {}
    }
}
```

Any input rate up to 192 kHz; internal working rate is 11 025 Hz.

## Tests

Run in release — the per-pixel DSP is far too slow unoptimized (upstream
CI does the same):

```sh
cargo test -p tempo-sstv --release
```

Real-radio validation evidence for the upstream baseline (ARISS Dec-2017
PD120/PD180 corpus, ARISS Fram2 Robot 36 corpus) is documented in
`tests/ariss_fram2_validation.md`; the fixture WAVs are not
redistributable and are not vendored.
