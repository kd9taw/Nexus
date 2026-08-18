// PSK sub-mode table — the sstvModes pattern: a pure data module the cockpit's
// mode selector renders from, so adding a mode is one row here plus its demod.
// COCKPIT STATE, not settings schema (the Keyboard Modes plan): the selector
// seam ships with one entry and QPSK31 slots in at Phase 3 without touching
// Settings, storage, or the backend surface.

export interface PskMode {
  /** Stable slug — the selector value (never shown raw). */
  slug: string
  /** Display name. */
  name: string
  /** Symbol rate (Bd) — shown in the badge, matches the demodulator. */
  baud: number
  /** One-line tooltip. */
  hint: string
}

export const PSK_MODES: PskMode[] = [
  {
    slug: 'psk31',
    name: 'PSK31',
    baud: 31.25,
    hint: 'BPSK at 31.25 Bd — the classic narrow-band keyboard ragchew mode (G3PLX varicode)',
  },
  {
    slug: 'qpsk31',
    name: 'QPSK31',
    baud: 31.25,
    hint:
      'Four-phase PSK31 with error correction (rate-1/2 K=5 code + soft Viterbi) — same speed, ' +
      'rides flutter and static crashes BPSK cannot; sideband-sensitive (see the Rev toggle)',
  },
]

export const PSK_MODE_BY_SLUG: Record<string, PskMode> = Object.fromEntries(
  PSK_MODES.map((m) => [m.slug, m]),
)
