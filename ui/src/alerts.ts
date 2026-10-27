// Decode alerts: a WebAudio beep + a visual toast for each station worth chasing.
//
// We do NOT alert on every decode — that's noise. Experienced operators want
// loud, aggressive alerts ONLY for the things worth chasing:
// - alertMyCall → a decode directed at my callsign (someone calling me)
// - alertNew    → a NEW DXCC entity (a "new one" — aggressive) or a new grid
// - alertCq     → a plain CQ (off by default; opt-in, since CQs are constant)
// - the watch list, above them all.
//
// ⭐ THE STATION DECIDES, THE SCREEN SHOWS. Which decode is worth an alert is decided once, at the
// station, beside the decodes (crates/tempo-app/src/needed_alert.rs): a computer driving the
// station must hear exactly what the station's own screen hears. It raises each one as the `alert`
// event (kind `needed`, `useStationAlerts`), and `showNeededAlert` below only renders it — the
// sound, the toast and its Work button. The shared table both ends are held to is
// crates/tempo-app/tests/fixtures/needed-alerts.json (`alerts.agreement.test.ts` here).
//
// What stays here is the eyes-free channel (`processDecodes`): the decode tick and the spoken
// batch summary are cues for the window that hears them, not alerts about a station.

//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every word an alert
// says comes from the catalog. What does NOT: the callsign, the DXCC entity name, the grid
// square and the watch filter's own label — they are interpolated as they arrive. The two
// optional clauses (` — <entity>` and the grid after "grid") are assembled here as WHOLE
// clauses and handed in, so a toast with neither never carries a dangling separator.

import type { DecodeRow, Settings, Tier } from './types'
import { pushToast } from './toast'
import { announce } from './announce'
import { t } from './i18n'

/** Test seam: forget the decode memory so a case starts from nothing. */
export function __resetAlertsForTest(): void {
  seenDecodes.clear()
}
// Every decode key ever seen (not just alert-worthy) — drives the batch
// freshness check for the decode tick + screen-reader batch summaries.
const seenDecodes = new Set<string>()

let audioCtx: AudioContext | null = null

/** Lazily create / resume the shared AudioContext (needs a user gesture first). */
export function ensureCtx(): AudioContext | null {
  try {
    if (!audioCtx) {
      const Ctor =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
      if (!Ctor) return null
      audioCtx = new Ctor()
    }
    if (audioCtx.state === 'suspended') void audioCtx.resume()
    return audioCtx
  } catch {
    return null
  }
}

/** Short two-tone beep. Frequencies differ by alert kind so they're distinguishable. */
export function beep(freq: number): void {
  const ctx = ensureCtx()
  if (!ctx) return
  const now = ctx.currentTime
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  osc.type = 'sine'
  osc.frequency.value = freq
  gain.gain.setValueAtTime(0.0001, now)
  gain.gain.exponentialRampToValueAtTime(0.18, now + 0.01)
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.22)
  osc.connect(gain)
  gain.connect(ctx.destination)
  osc.start(now)
  osc.stop(now + 0.24)
}

function decodeKey(d: DecodeRow): string {
  return `${d.from ?? '?'}|${d.message}|${Math.round(d.freqHz)}`
}

const BEEP_HZ = { mycall: 880, newdxcc: 520, newgrid: 740, cq: 620 }

/** Two quick tones — the attention-grabbing alert for a new DXCC ("new one")
 * and the M5+/X-class solar-flare heads-up (flareAlert.ts). */
export function doubleBeep(freq: number): void {
  beep(freq)
  window.setTimeout(() => beep(freq * 1.5), 130)
}

/** Soft, short tick — much quieter than an alert beep. The decode-batch pulse
 * (Settings, off by default): the band's rhythm for eyes-free operating. */
function tickBeep(): void {
  const ctx = ensureCtx()
  if (!ctx) return
  const now = ctx.currentTime
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  osc.type = 'sine'
  osc.frequency.value = 340
  gain.gain.setValueAtTime(0.0001, now)
  gain.gain.exponentialRampToValueAtTime(0.05, now + 0.008)
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.07)
  osc.connect(gain)
  gain.connect(ctx.destination)
  osc.start(now)
  osc.stop(now + 0.08)
}

/** TX key/unkey earcon (Settings `soundTxState`, off by default): a rising
 * two-tone on key-down, a falling one on unkey — the eyes-free TX pill. */
export function txEarcon(on: boolean): void {
  if (on) {
    beep(660)
    window.setTimeout(() => beep(880), 110)
  } else {
    beep(880)
    window.setTimeout(() => beep(560), 110)
  }
}

/** The CW turn cue's sound (Settings `soundCwTurn`, off by default): one short rising chirp,
 * 1.2 → 1.6 kHz in 120 ms, quieter than an alert and nothing like a ~600 Hz CW note. The cockpit
 * plays it once per over, and never while anything is keyed. */
export function turnEarcon(): void {
  const ctx = ensureCtx()
  if (!ctx) return
  const now = ctx.currentTime
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  osc.type = 'sine'
  osc.frequency.setValueAtTime(1200, now)
  osc.frequency.exponentialRampToValueAtTime(1600, now + 0.12)
  gain.gain.setValueAtTime(0.0001, now)
  gain.gain.exponentialRampToValueAtTime(0.08, now + 0.01)
  gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.12)
  osc.connect(gain)
  gain.connect(ctx.destination)
  osc.start(now)
  osc.stop(now + 0.13)
}

/** Pass-start (AOS) earcon for the ARMED satellite track (satPassAlert.ts): a
 * rising three-tone. Three notes, not two, and frequencies no other alert
 * uses (BEEP_HZ, the 990 Hz ⏰ alarm, the 660/880 TX pair) — the operator's
 * hands are on a rotor and the sound alone must say which event this is. */
export function aosEarcon(): void {
  beep(523)
  window.setTimeout(() => beep(784), 120)
  window.setTimeout(() => beep(1046), 240)
}

/** Pass-end (LOS) earcon — the falling mirror of [`aosEarcon`], so rise and
 * set are tellable apart blind. */
export function losEarcon(): void {
  beep(1046)
  window.setTimeout(() => beep(784), 120)
  window.setTimeout(() => beep(523), 240)
}

/** What the operator is currently doing in the FT8/FT4 sequencer — lets the
 * alerts stay quiet while they're already engaged (the chatty-popup fix). */
export interface QsoContext {
  /** Sequencer state ("Listening", "CallingCq", "AwaitReport", … "Done"), or
   * null when the FT8 area isn't active at all. */
  state: string | null
  /** The station currently being worked, if any. */
  dxcall: string | null
}

/**
 * Engaged = the sequencer is MID-QSO. Calling CQ is not mid-QSO, and the difference is the
 * whole point of this function.
 *
 * ⚠️ `CallingCq` COUNTED AS ENGAGED UNTIL 2026-08-22, which silenced the one alert an operator
 * calling CQ is actually waiting for: `mycall` is gated on `!engaged`, so a station answering
 * YOU raised no beep at all. Reported by a new operator whose other alerts all worked ("the
 * other alerts like new DXCC work fine") — the tell that audio was fine and this single path
 * was gated off. WSJT-X alerts here, and an operator running CQ with Auto OFF has nothing else
 * to tell them somebody replied.
 *
 * The guard is still right for a QSO in progress — that was the chatty-popup fix, and `mycall`
 * dedups per DECODE, so without it every message of an exchange would beep. Nothing is lost by
 * exempting CallingCq: the moment somebody answers, the sequencer leaves CallingCq for
 * AwaitReport (or the Field Day equivalent) and the rest of the exchange is suppressed exactly
 * as before. So the alert fires on the ANSWER and then goes quiet.
 *
 * The engine draws the same line: `qso.rs` scores `State::Listening | State::CallingCq => 0`
 * exchanges completed, grouping the two precisely as here.
 */
export function engagedInQso(ctx?: QsoContext): boolean {
  return (
    !!ctx?.state &&
    ctx.state !== 'Listening' &&
    ctx.state !== 'CallingCq' &&
    ctx.state !== 'Done'
  )
}

/** Per-alert band scope: which bands an alert type may fire on. 'vhf' = 6 m and up
 * (≥ 50 MHz — where grid chasing actually lives, VUCC/FFMA), 'hf' = below 6 m. */
export type AlertBandScope = 'off' | 'hf' | 'vhf' | 'all'

/**
 * Does an alert scope admit the current dial? Unknown dial (no CAT) is PERMISSIVE —
 * any non-off scope passes, because silently dropping alerts on a band we can't
 * verify would hide real ones; the operator can still pick Off for hard silence.
 */
export function bandScopeOk(scope: string | undefined, dialMhz: number | undefined, fallback: AlertBandScope): boolean {
  const s = (scope ?? fallback) as AlertBandScope
  if (s === 'off') return false
  if (s === 'all') return true
  if (dialMhz == null || !(dialMhz > 0)) return true // band unknown → permissive
  return s === 'vhf' ? dialMhz >= 50 : dialMhz < 50
}

/**
 * The eyes-free channel of a look at the decode window: rows never seen before (any kind, not
 * just alert-worthy) sound the soft decode tick (`soundDecodeTick`) and, at announce verbosity
 * "all", a spoken batch summary — both rate-limited by construction to one event per decode
 * cycle. The alerts about a station are the station's (`showNeededAlert`). The seen-set is capped
 * so a long session can't grow it unbounded (a reset only risks one duplicate tick).
 */
export function processDecodes(decodes: DecodeRow[], settings: Settings): void {
  if (seenDecodes.size > 5000) seenDecodes.clear()
  const fresh: DecodeRow[] = []
  for (const d of decodes) {
    if (d.mine) continue
    const k = decodeKey(d)
    if (!seenDecodes.has(k)) {
      seenDecodes.add(k)
      fresh.push(d)
    }
  }
  if (fresh.length > 0) {
    if (settings.soundDecodeTick) tickBeep()
    // Verbosity "all": a compact spoken batch summary (count + up to three CQ
    // callers) — the QLog filter doctrine, never the raw firehose. "needed"
    // adds nothing here: the alert toasts are already announced.
    if (settings.announceVerbosity === 'all') {
      const cqs = fresh.filter((d) => d.isCq && d.from).slice(0, 3)
      const cqPart = cqs.length
        ? t('alerts.batch.cq', {
            calls: cqs.map((d) => `${d.from}${d.country ? ` (${d.country})` : ''}`).join(', '),
          })
        : ''
      announce(t('alerts.batch.decodes', { count: fresh.length }) + cqPart)
    }
  }
}

/** A needed-station alert as the station raises it (the `alert` event, kind `needed`): its
 * toast's parts, and the decode its Work button works. `NeededAlert` in
 * crates/tempo-app/src/needed_alert.rs, whose serde spelling this is. */
export interface NeededAlert {
  kind: 'watch' | 'newDxcc' | 'rareGrid' | 'newGrid' | 'myCall' | 'cq'
  /** `null` for a station not parsed: the toast says "a station", and has no button. */
  call: string | null
  entity: string | null
  /** A rare grid's square. */
  grid: string | null
  /** A rare grid's tier. */
  tier: 'rare' | 'ultraRare' | null
  /** A watch-list alert's entry, as the operator labelled it. */
  watch: string | null
  work: { message: string; snr: number; freqHz: number; tier: Tier } | null
}

/**
 * Show one needed-station alert: its sound and its toast, with a button that works the station
 * (identical to double-clicking its decode row) when `onWork` is given and the station is known.
 * Decides nothing — the station already did.
 *
 * Loudness by kind: a watch-list hit and a new DXCC are the aggressive double tone with a long,
 * prominent toast; a rare (islet/sliver) or ultra-rare (water-only) grid earns the same at its own
 * pitch; someone calling YOU is the most time-critical — a beep and the longest toast, so it is
 * still there when you look; a plain new grid is a quiet info toast with no beep (the "too chatty"
 * fix); an opted-in CQ is a beep and a short info toast.
 */
export function showNeededAlert(a: NeededAlert, onWork?: (a: NeededAlert) => void): void {
  const who = a.call ?? t('alerts.station')
  const where = a.entity ? t('alerts.where', { entity: a.entity }) : ''
  const action = onWork && a.call ? () => onWork(a) : undefined
  const work = t('alerts.action.work')
  switch (a.kind) {
    case 'watch':
      doubleBeep(BEEP_HZ.newdxcc)
      pushToast(t('alerts.watch', { what: a.watch ?? '', call: who, where }), 'success', 15000, {
        alert: true,
        prominent: true,
        action,
        actionLabel: work,
      })
      return
    case 'newDxcc':
      doubleBeep(BEEP_HZ.newdxcc)
      pushToast(t('alerts.newDxcc', { call: who, where }), 'success', 15000, {
        alert: true,
        prominent: true,
        action,
        actionLabel: work,
      })
      return
    case 'rareGrid': {
      const tier = a.tier === 'ultraRare' ? t('alerts.tier.ultraRare') : t('alerts.tier.rare')
      doubleBeep(BEEP_HZ.newgrid)
      pushToast(
        t('alerts.rareGrid', { tier, grid: a.grid ? ` ${a.grid}` : '', call: who, where }),
        'success',
        15000,
        { alert: true, prominent: true, action, actionLabel: work },
      )
      return
    }
    case 'newGrid':
      pushToast(t('alerts.newGrid', { call: who, where }), 'info', 6000, {
        alert: true,
        action,
        actionLabel: work,
      })
      return
    case 'myCall':
      beep(BEEP_HZ.mycall)
      pushToast(t('alerts.myCall', { call: who }), 'success', 20000, {
        alert: true,
        prominent: true,
        action,
        actionLabel: t('alerts.action.answer'),
      })
      return
    case 'cq':
      beep(BEEP_HZ.cq)
      pushToast(t('alerts.cq', { call: who, where }), 'info', 6000, {
        alert: true,
        action,
        actionLabel: t('alerts.action.answer'),
      })
      return
  }
}

/** A park newly on the air (the station's new-activation alert, kind `parks`): one double beep
 * however many parks the look found. */
export function showFreshParks(): void {
  doubleBeep(880)
}
