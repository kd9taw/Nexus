# ARISS Fram2 Real-Audio Validation Procedure

The ARISS Fram2 mission (April 2025) used Robot 36 and ARISS-USA published 12
reference WAV + JPG pairs for decoder validation:
<https://ariss-usa.org/ARISS_SSTV/Fram2Test/>.

This is the merge gate for V2.2 (Robot family decoding) and the regression
net for any future change to `mode_robot.rs` or the `ChannelLayout::RobotYuv`
dispatch.

## Prerequisites

1. Pull the 12 WAV + 12 JPG pairs into `tests/fixtures/ariss-fram2/`
   (gitignored corpus — license unclear for redistribution):

   ```bash
   mkdir -p tests/fixtures/ariss-fram2
   # Subshell so the `cd` doesn't leak into Step 2's `cargo build`.
   (
     cd tests/fixtures/ariss-fram2
     for n in 01 02 03 04 05 06 07 08 09 10 11 12; do
       # -f fail-on-HTTP-error (no bogus error-page files), -S show errors
       # in -s mode, -L follow redirects, -O save with the URL's basename.
       curl -fSLO "https://ariss-usa.org/ARISS_SSTV/Fram2Test/Slide${n}.wav"
       curl -fSLO "https://ariss-usa.org/ARISS_SSTV/Fram2Test/Slide${n}-Robot-36-Color.jpg"
     done
   )
   ```

2. Build slowrx-cli in release mode:

   ```bash
   cargo build --release --features cli
   ```

## Run validation

Decode all 12 WAVs and inspect each PNG against its reference JPG:

```bash
set -e
mkdir -p /tmp/fram2-validate
# Clear any stale outputs from a previous run so a regressed re-run
# can't look like 12/12 by leaving old PNGs in place.
rm -f /tmp/fram2-validate/Slide*-decoded.png /tmp/fram2-validate/img-001-*.png

for n in 01 02 03 04 05 06 07 08 09 10 11 12; do
  # Capture the CLI's stdout+stderr to a temp file so we can both display
  # the last 2 lines AND check the exit status (piping through `tail`
  # would mask the decoder's exit status).
  log=$(mktemp)
  if ! ./target/release/slowrx-cli \
       --input "tests/fixtures/ariss-fram2/Slide${n}.wav" \
       --output "/tmp/fram2-validate/" >"$log" 2>&1; then
    echo "Slide${n}: slowrx-cli FAILED" >&2
    tail -10 "$log" >&2
    rm "$log"
    exit 1
  fi
  tail -2 "$log"
  rm "$log"
  # Rename so each output is identifiable.
  if [ -f "/tmp/fram2-validate/img-001-robot36.png" ]; then
    mv "/tmp/fram2-validate/img-001-robot36.png" \
       "/tmp/fram2-validate/Slide${n}-decoded.png"
  else
    echo "Slide${n}: NO PNG PRODUCED" >&2
    exit 1
  fi
done
ls -la /tmp/fram2-validate/
```

Each WAV should produce: `1 VIS, 240 lines, 1 image(s)` and a PNG file.
The script fails closed if any decode fails or any PNG is missing — a
regressed re-run will exit non-zero, NOT silently leave stale PNGs that
look like a 12/12 pass.

Open both directories in an image viewer and visually compare each
`Slide${n}-decoded.png` against the reference
`tests/fixtures/ariss-fram2/Slide${n}-Robot-36-Color.jpg`.

## Acceptance criteria (visual match)

For all 12 slides:

1. **Recognizable image content.** The same scene appears in both —
   subjects, text, shapes are identifiable.

2. **Color accuracy within slowrx tolerances.** Hue and brightness
   approximately match. Slight tonal drift is acceptable; obvious color
   casts (everything blue / red / green) are NOT acceptable.

3. **Geometric integrity.** Image dimensions are 320×240. The image
   isn't distorted or shifted by many pixels. Slight slant (a few pixels
   per line) is acceptable if the reference shows it too; large slant
   is not.

4. **No black/blank rows or columns.** Each row decoded should have
   visible content. Occasional sync slips on noisy lines (1–2 per image)
   are acceptable.

If a slide fails any criterion above, do NOT merge V2.2. Investigate.

## Common failure modes and where to look

- **All 12 images upside-down or scrambled colors:** chroma channel
  swap (Cr/Cb confusion) in `mode_robot::decode_r36_or_r24_line`. Check
  the row-parity-to-chroma-channel mapping — slowrx C: even row → ch1
  (Cr), odd row → ch2 (Cb).

- **Pixel time off — image squished horizontally:** the `pixel_seconds * 2`
  factor for the Y channel decode is wrong. Re-read slowrx
  `video.c:60-70` R36/R24 case ChanLen[0] — should be PixelTime ×
  Width × 2.

- **Top row has wrong/missing color:** expected — image[0] never gets
  Cb written (slowrx C does the same; chroma duplication propagates
  forward only). If row 0 is solid green/blue, that's the zero-init Cb
  showing through. Acceptable; documented in `mode_robot.rs::decode_r36_or_r24_line`.

- **Bottom half of every image is solid green / zero-init:** This was
  the V2.2 Phase 5 surprise. Root cause: `target_audio_samples` formula
  was unconditionally PD-style (`image_lines / 2 × line_seconds`),
  which under-buffers Robot by half (Robot has no PD-style line
  pairing). The decoder's find_sync triggered after only half the
  audio was buffered; the bottom-half rows read past the end of
  `d.audio` and got `Y=0`, which composes via `ycbcr_to_rgb(0,0,0)` to
  the green `[0, 133, 0]` color. Fix: branch on `spec.channel_layout`
  in `target_audio_samples` per slowrx C `video.c:251-254`. Landed in
  commit `b0ad941`. If you see this again, the formula has regressed.

- **Decoder panics or crashes:** likely chroma-duplicate bounds guard
  broken — `line_index + 1 == image_lines` should be a no-op. Check the
  guard at `decode_r36_or_r24_line`.

- **VIS not detected (`0 VIS`):** unlikely after V2.2 Phase 1 — but
  re-run `cargo test --lib robot36_vis_code_resolves` to confirm the
  modespec lookup is intact.

## Pass/fail recording

After validation, append a one-line note to `CHANGELOG.md` under the
`[0.3.0]` section noting the date and the result. Example:

> Validated against 12 ARISS Fram2 Robot 36 captures on 2026-05-XX —
> all 12 produced visually-matching images.

Or, if some failed and were fixed, document the iteration:

> Validated against 12 ARISS Fram2 Robot 36 captures on 2026-05-XX
> after one decoder fix (commit XXXXXXX).

## Re-running this validation later

Any change to `mode_robot.rs`, `mode_pd.rs::decode_one_channel_into`,
`decoder.rs` dispatch, or `vis.rs` should re-run this procedure. The
synthetic round-trip (`tests/roundtrip.rs`) is necessary but not
sufficient — real-audio validation is the load-bearing check, per the
project's "real-audio fixture match is a merge gate when fixtures are
available" policy.
