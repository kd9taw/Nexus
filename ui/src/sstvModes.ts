/**
 * The SSTV transmit-mode table — the ONE copy on the TypeScript side.
 *
 * ⚠️ THIS TABLE IS A MIRROR, AND `crates/tempo-sstv/src/modespec.rs` IS THE ARBITER.
 * `sstv-modes.test.ts` parses both sides and compares them — dimensions against
 * `ModeSpec`, seconds against the encoder's own `tx_duration_secs` formula. Nothing
 * here may be edited without that guard agreeing: the transmit path reads the Rust
 * table and REFUSES anything that is not exactly `line_pixels × image_lines`, so a
 * drift here is a resizer producing a picture the backend will not send.
 *
 * It lives in its own module — no React, no `../api` — because two surfaces now need
 * it: the SSTV cockpit's per-picture picker and the Settings default-mode picker.
 * Importing `SstvView.tsx` from `SettingsPanel.tsx` to reach it would drag the whole
 * cockpit (canvas, waterfall, every SSTV verb) into the panel's module graph, and
 * every SettingsPanel test mocks `../api` with an explicit verb list. A hand copy in
 * SettingsPanel is the other wrong answer: two tables describing the same 15 modes
 * with nothing comparing them is exactly what `sstv-modes.test.ts` exists to stop.
 */

/** One transmittable SSTV mode: the backend `parse_sstv_mode` slug, its display
 * name, its exact pixel dimensions (the composer resizes to these; the backend
 * refuses any mismatch), and the EXACT on-air key-down time. */
export interface TxMode {
  slug: string
  name: string
  group: 'Scottie' | 'Martin' | 'Robot' | 'PD'
  width: number
  height: number
  /** Exact key-down seconds: header + scanlines, rounded. NOT approximate — the
   *  composer tells the operator how long the rig is keyed, and every entry here
   *  used to be about a second short of what the encoder actually emits. */
  seconds: number
}

/** The 15 modes, grouped by family. Five distinct rasters and five distinct aspect
 * ratios, which is why the crop box re-derives its shape on every mode change and
 * not just its pixel size. */
export const SSTV_TX_MODES: TxMode[] = [
  { slug: 'scottie1', name: 'Scottie 1', group: 'Scottie', width: 320, height: 256, seconds: 111 },
  { slug: 'scottie2', name: 'Scottie 2', group: 'Scottie', width: 320, height: 256, seconds: 72 },
  { slug: 'scottiedx', name: 'Scottie DX', group: 'Scottie', width: 320, height: 256, seconds: 270 },
  { slug: 'martin1', name: 'Martin 1', group: 'Martin', width: 320, height: 256, seconds: 115 },
  { slug: 'martin2', name: 'Martin 2', group: 'Martin', width: 320, height: 256, seconds: 59 },
  { slug: 'robot24', name: 'Robot 24', group: 'Robot', width: 320, height: 240, seconds: 37 },
  { slug: 'robot36', name: 'Robot 36', group: 'Robot', width: 320, height: 240, seconds: 37 },
  { slug: 'robot72', name: 'Robot 72', group: 'Robot', width: 320, height: 240, seconds: 73 },
  { slug: 'pd50', name: 'PD-50', group: 'PD', width: 320, height: 256, seconds: 51 },
  { slug: 'pd90', name: 'PD-90', group: 'PD', width: 320, height: 256, seconds: 91 },
  { slug: 'pd120', name: 'PD-120', group: 'PD', width: 640, height: 496, seconds: 127 },
  { slug: 'pd160', name: 'PD-160', group: 'PD', width: 512, height: 400, seconds: 162 },
  { slug: 'pd180', name: 'PD-180', group: 'PD', width: 640, height: 496, seconds: 188 },
  { slug: 'pd240', name: 'PD-240', group: 'PD', width: 640, height: 496, seconds: 249 },
  { slug: 'pd290', name: 'PD-290', group: 'PD', width: 800, height: 616, seconds: 290 },
]

/** Family order for the grouped pickers — both of them render `<optgroup>`s in this
 *  order, so the cockpit and Settings read the same way. */
export const TX_MODE_GROUPS: TxMode['group'][] = ['Scottie', 'Martin', 'Robot', 'PD']

/** Slug → mode. Also the validity test both surfaces use: a slug that is not a key
 *  here is one this build cannot send (a hand-edited or downgraded settings file). */
export const MODE_BY_SLUG: Record<string, TxMode> = Object.fromEntries(
  SSTV_TX_MODES.map((m) => [m.slug, m]),
)
