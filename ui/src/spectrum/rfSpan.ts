// THE RF PANADAPTER'S SPANS — what the Phone and CW cockpits' span chips draw on a native RF row, and
// what each source is asked to sweep so the window is drawn at the finest resolution it can deliver.
//
// THE REPORT THIS ANSWERS: on 30 m, Full showed 9.1–11.1 MHz for a band that is 10.100–10.150, and
// ±25 kHz looked pixelated. Full is the source's whole sweep — an SDR's 2 MHz IQ there — and the ±
// chips only cropped it: 2048 bins over 2 MHz is 977 Hz a bin, so ±25 kHz was ~51 bins stretched
// across the scope. Two changes, one per cause:
//   - BAND fits the band the dial is in, edge to edge with a small margin (`bandWindow`), so 30 m
//     draws 10.095–10.155 MHz.
//   - The window is ASKED FOR rather than cropped. A radio whose sweep Nexus can set is asked for the
//     narrowest sweep that holds it (`spanAsk`: Icom CI-V `27 15`, the FlexRadio pan's `bandwidth`); an
//     SDR's IQ panadapter is handed the window on every poll and computes its rows over it
//     (PhoneScope's `wantedRf`, `tempo-audio` `tci::iq`'s zoom). A source that can do neither draws
//     what it sweeps and the cockpit says when that is only part of the band.
//
// THE FT-710 IS NOT HERE. Its own position ladder already has the band fit: FIX asks the radio for
// the narrowest rung that covers the tuned band and pins the sweep to its lower edge
// (`tempo-audio` `yaesu_wf::auto_fix_span_code`).
//
// NOTHING HERE TRANSMITS OR TOUCHES A TRANSMIT SETTING: a scope span is a display setting on every
// radio it is sent to, and nothing is sent while the station is on the air.
import { useEffect, useRef } from 'react'
import { bandLabelForMhz, bandRangeForLabel } from '../band'
import { surfaceGet, surfaceSet } from '../features/windowScope'

/** The margin either side of a band: a tenth of its width, at most this. 30 m's 50 kHz gets 5 kHz a
 *  side; 20 m's 350 kHz, 6 m's and 2 m's 4 MHz get 25 kHz. */
export const BAND_MARGIN_FRACTION = 0.1
export const BAND_MARGIN_MAX_HZ = 25_000

/** The band `dialHz` is in (`band.ts`, the ADIF band identity the rest of the app labels with) and
 *  the window that fits it: its edges, each widened by the margin. Null off every amateur band. */
export function bandWindow(dialHz: number | null): { band: string; loHz: number; hiHz: number } | null {
  if (dialHz == null || !(dialHz > 0)) return null
  const band = bandLabelForMhz(dialHz / 1e6)
  const edges = band ? bandRangeForLabel(band) : null
  if (!edges) return null
  // The table is MHz; whole hertz keep 10.1 × 1e6 from reading 10100000.000000002.
  const lo = Math.round(edges.lo * 1e6)
  const hi = Math.round(edges.hi * 1e6)
  const margin = Math.min(BAND_MARGIN_MAX_HZ, (hi - lo) * BAND_MARGIN_FRACTION)
  return { band, loHz: lo - margin, hiHz: hi + margin }
}

/** Icom's centre-mode spans (CI-V `27 15`), ± half-width in Hz: the radio's own table (IC-7300 Full
 *  Manual §19, IC-9700 CI-V Reference Guide, command 27 15). A value off the table is refused. */
export const ICOM_HALF_SPANS = [2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000] as const

/** What a source's own sweep is asked for: Icom's ± half-width rung, or the FlexRadio pan's whole
 *  bandwidth (SmartSDR `display pan set … bandwidth=`, any width; Nexus's pan is its own). */
export type SpanAsk = { kind: 'civ'; halfHz: number } | { kind: 'flex'; hz: number }

/** The sweep `source` must be set to so `want` is all inside it. Both sources sweep CENTRED ON THE
 *  DIAL here, so the half-width needed is the dial's distance to the farther edge. Icom: the
 *  narrowest rung that reaches it, or the widest it has. Flex: twice that, to the next kHz. Null for a
 *  source whose sweep Nexus does not set (TCI and SDRconnect are zoomed instead; the FT-710 has FIX),
 *  or with no dial to centre on. */
export function spanAsk(
  source: string,
  dialHz: number | null,
  want: { loHz: number; hiHz: number } | null,
): SpanAsk | null {
  if (!want || dialHz == null || !(dialHz > 0)) return null
  const need = Math.max(dialHz - want.loHz, want.hiHz - dialHz)
  if (!(need > 0)) return null
  if (source === 'civ') {
    const rung = ICOM_HALF_SPANS.find((h) => h >= need) ?? ICOM_HALF_SPANS[ICOM_HALF_SPANS.length - 1]
    return { kind: 'civ', halfHz: rung }
  }
  if (source === 'flex') return { kind: 'flex', hz: Math.ceil((2 * need) / 1000) * 1000 }
  return null
}

/** The whole width (Hz) an ask sweeps. */
const askWidth = (a: SpanAsk) => (a.kind === 'civ' ? 2 * a.halfHz : a.hz)

/** How much wider than the ask a sweep that already holds the window may be and be left alone: one
 *  Icom rung (they step by 2–2.5×). Without the slack a dial tuned back and forth across the point
 *  where the rung changes would set the radio's span on every step. */
const LEAVE_WIDER = 2.5

/** Does a sweep of `loHz..hiHz` already do what `ask` would: hold `want` and be no wider than one
 *  rung over the ask? A sweep the radio reports a hair short of an edge still counts (a bin's slack). */
export function sweepFits(
  sweep: { loHz: number; hiHz: number },
  want: { loHz: number; hiHz: number },
  ask: SpanAsk,
): boolean {
  const width = sweep.hiHz - sweep.loHz
  const slack = width / 400
  return (
    sweep.loHz <= want.loHz + slack &&
    sweep.hiHz >= want.hiHz - slack &&
    width <= askWidth(ask) * LEAVE_WIDER
  )
}

/** Is `want` only partly inside what is drawn (`drawn`, the clamped window)? Said under the chips. */
export function onlyPart(
  drawn: { loHz: number; hiHz: number } | null,
  want: { loHz: number; hiHz: number } | null,
): boolean {
  if (!drawn || !want) return false
  const slack = (want.hiHz - want.loHz) / 100
  return drawn.loHz > want.loHz + slack || drawn.hiHz < want.hiHz - slack
}

/** A sweep as the scope reported it, with the radio (the active radio's id) and source it was
 *  reported under. */
export type TaggedSweep = { loHz: number; hiHz: number; radio?: number; source: string }

/** The reported sweep if it is `radio`'s on `source`, else null. A cockpit keeps the last sweep
 *  reported, so after a radio swap (or the source changing, Icom to Flex) it still holds the old
 *  one until the new radio's first row: that sweep says nothing about the new radio's span. */
export function sweepFor(
  tagged: TaggedSweep | null,
  radio: number | undefined,
  source: string | null,
): { loHz: number; hiHz: number } | null {
  if (!tagged || tagged.radio !== radio || tagged.source !== source) return null
  return { loHz: tagged.loHz, hiHz: tagged.hiHz }
}

/** Two sweep widths within this fraction of each other are the same width: a radio reporting the
 *  span it was asked for a few hertz off is still on it. Icom's rungs are 2x apart and more. */
const SAME_WIDTH = 0.01

/** PUT THE RADIO'S SWEEP ON THE CHOSEN WINDOW, ONCE: send `ask` when the sweep the source reports
 *  does not already fit `want`, ONCE FOR EACH ASK. An ask is the radio (`radio`, the active radio's
 *  id), the source, the Icom rung, the band the dial is in and the span chip chosen (`choice`): a
 *  new radio, band, chip or rung is a new ask. The Flex pan's width is not in it: that width follows
 *  the dial hertz by hertz, so tuning would make every few hundred hertz a new ask.
 *
 *  Under one ask, the sweep's WIDTH says who moved it. The width the ask set (or, for an ask the
 *  sweep already fitted when first seen, the width it had then) is Nexus's; once a sweep of that
 *  width has fitted, a sweep of the same width that stops fitting because the window now needs a
 *  WIDER sweep means the DIAL moved, and it is asked again (a row that only lags the dial by a
 *  poll misses an edge at the same width, and is not). Any other width is the operator's or
 *  another window's, and stands: a span set on the radio's own knob, the Rig Scope chips or a Flex
 *  pan chip, a radio that refuses (an Icom in Fixed mode, whose sweep never fits after the ask)
 *  told once, and two scopes asking different spans of one radio each asking once rather than
 *  taking turns undoing the other. Nothing is sent before the source has reported a sweep, while
 *  transmitting (the ask waits for unkey), or for a source whose sweep Nexus does not set.
 *
 *  `sweep` must be THIS radio's and this source's (`sweepFor`): another radio's sweep left in the
 *  cockpit would be read as the new ask's first sight. */
export function useSpanAsk({
  radio,
  source,
  dialHz,
  want,
  choice,
  sweep,
  transmitting,
  send,
}: {
  radio?: number
  source: string | null
  dialHz: number | null
  want: { loHz: number; hiHz: number } | null
  choice: string
  sweep: { loHz: number; hiHz: number } | null
  transmitting: boolean
  send: (ask: SpanAsk) => void
}): void {
  // The ask last seen, the sweep width that is Nexus's under it, and whether a sweep of that width
  // has fitted since.
  const spent = useRef<{ key: string; width: number; fitted: boolean } | null>(null)
  const sendRef = useRef(send)
  sendRef.current = send
  const ask = source ? spanAsk(source, dialHz, want) : null
  const band = dialHz != null && dialHz > 0 ? bandLabelForMhz(dialHz / 1e6) : ''
  const rung = ask?.kind === 'civ' ? ask.halfHz : ''
  const key = ask ? `${radio ?? ''}:${source}:${ask.kind}:${rung}:${band}:${choice}` : ''
  const fits = ask != null && want != null && sweep != null && sweepFits(sweep, want, ask)
  const width = sweep ? sweep.hiHz - sweep.loHz : 0
  useEffect(() => {
    if (!ask || !sweep || transmitting) return
    const s = spent.current
    if (s?.key === key) {
      if (Math.abs(width - s.width) > s.width * SAME_WIDTH) return
      if (fits) s.fitted = true
      // Only a sweep narrower than the ask is the dial moving: both sources sweep centred on the
      // dial, and a row a poll behind the dial misses an edge by a step while being exactly as wide.
      if (fits || !s.fitted || askWidth(ask) <= s.width) return
    } else if (fits) {
      spent.current = { key, width, fitted: true }
      return
    }
    spent.current = { key, width: askWidth(ask), fitted: false }
    sendRef.current(ask)
    // `ask` is a fresh object every render; what decides a send is in the deps, and a send takes
    // the ask of the render that ran it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, fits, width, sweep != null, transmitting])
}

/** Where the chosen span is kept: one record per cockpit PER WINDOW, like the audio scope's span
 *  chips, so a torn-off cockpit may run Band beside a ±5 kHz docked one. A stored choice wins over the
 *  default. The main window's key is the bare one, which is also the spelling an earlier build stored
 *  for the whole station, so that choice is read where it was left; a pop-out with no choice of its
 *  own reads the main window's (`surfaceGet`). Static literals: the storage-scope test classifies
 *  them by these declarations. */
const PHONE_RF_SPAN_KEY = 'nexus.phone.rfSpan'
const CW_RF_SPAN_KEY = 'nexus.cw.rfSpan'

/** The stored span id for `cockpit` in this window if it is one of `ids`, else `fallback`.
 *  Storage-safe: `surfaceGet` reads a refused store as nothing stored. */
export function loadRfSpan<T extends string>(cockpit: 'phone' | 'cw', ids: readonly T[], fallback: T): T {
  const v = cockpit === 'phone' ? surfaceGet(PHONE_RF_SPAN_KEY) : surfaceGet(CW_RF_SPAN_KEY)
  return ids.find((id) => id === v) ?? fallback
}

/** Keep `id` as this window's span for `cockpit`. Never touches another window's record. */
export function saveRfSpan(cockpit: 'phone' | 'cw', id: string): void {
  if (cockpit === 'phone') surfaceSet(PHONE_RF_SPAN_KEY, id)
  else surfaceSet(CW_RF_SPAN_KEY, id)
}
